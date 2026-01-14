# FUSE Handler Integration Proposal

> **NOTA**: Esta documentacao e conceitual. As mudancas reais no `fuse.rs` devem ser feitas durante a fase de implementacao.

## Objetivo

Integrar o `HandlerRegistry` no `AgentFSFuse` para permitir handlers extensiveis que interceptam operacoes FUSE.

## Mudancas Propostas em `cli/src/fuse.rs`

### 1. Adicionar HandlerRegistry ao AgentFSFuse

```rust
use crate::handler::{HandlerRegistry, DefaultHandler};

struct AgentFSFuse {
    fs: Arc<dyn FileSystem>,
    runtime: Runtime,
    path_cache: Arc<Mutex<HashMap<u64, String>>>,
    open_files: Arc<Mutex<HashMap<u64, OpenFile>>>,
    next_fh: AtomicU64,
    uid: u32,
    gid: u32,
    mountpoint_path: String,
    // NOVO: Handler registry para operacoes extensiveis
    handler_registry: Arc<HandlerRegistry>,
}
```

### 2. Inicializacao com Registry

```rust
impl AgentFSFuse {
    pub fn new(
        fs: Arc<dyn FileSystem>,
        options: &FuseMountOptions,
        handler_registry: Option<Arc<HandlerRegistry>>,
    ) -> Self {
        let runtime = Runtime::new().expect("Failed to create Tokio runtime");

        // Usar registry fornecido ou criar um default
        let handler_registry = handler_registry.unwrap_or_else(|| {
            Arc::new(HandlerRegistry::with_filesystem(fs.clone()))
        });

        Self {
            fs,
            runtime,
            path_cache: Arc::new(Mutex::new(HashMap::new())),
            open_files: Arc::new(Mutex::new(HashMap::new())),
            next_fh: AtomicU64::new(1),
            uid: options.uid.unwrap_or_else(|| unsafe { libc::getuid() }),
            gid: options.gid.unwrap_or_else(|| unsafe { libc::getgid() }),
            mountpoint_path: options.mountpoint.to_string_lossy().into_owned(),
            handler_registry,
        }
    }

    /// Registrar um handler adicional
    pub fn register_handler(&mut self, handler: Arc<dyn FileHandler>) {
        Arc::get_mut(&mut self.handler_registry)
            .expect("Cannot modify shared registry")
            .register(handler);
    }
}
```

### 3. Modificar `getattr` para usar Registry

```rust
fn getattr(&mut self, _req: &Request, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
    tracing::debug!("FUSE::getattr: ino={}", ino);

    let path = if ino == 1 {
        "/".to_string()
    } else {
        match self.path_cache.lock().get(&ino) {
            Some(p) => p.clone(),
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        }
    };

    // ANTES: Chamada direta ao filesystem
    // let result = self.runtime.block_on(async {
    //     self.fs.lstat(&path).await
    // });

    // DEPOIS: Usar handler registry
    let registry = self.handler_registry.clone();
    let result = self.runtime.block_on(async move {
        registry.handle_getattr(&path).await
    });

    match result {
        Ok(Some(stats)) => {
            let attr = fillattr(&stats, self.uid, self.gid);
            reply.attr(&TTL, &attr);
        }
        Ok(None) => reply.error(libc::ENOENT),
        Err(e) => reply.error(error_to_errno(&e)),
    }
}
```

### 4. Modificar `read` para usar Registry

```rust
fn read(
    &mut self,
    _req: &Request,
    ino: u64,
    fh: u64,
    offset: i64,
    size: u32,
    _flags: i32,
    _lock_owner: Option<u64>,
    reply: ReplyData,
) {
    tracing::debug!("FUSE::read: ino={}, fh={}, offset={}, size={}", ino, fh, offset, size);

    // Obter path do inode
    let path = match self.path_cache.lock().get(&ino) {
        Some(p) => p.clone(),
        None => {
            // Fallback para file handle se path nao esta em cache
            let open_files = self.open_files.lock();
            if let Some(open_file) = open_files.get(&fh) {
                let file = open_file.file.clone();
                drop(open_files);

                let result = self.runtime.block_on(async move {
                    file.pread(offset as u64, size as u64).await
                });

                match result {
                    Ok(data) => reply.data(&data),
                    Err(e) => reply.error(error_to_errno(&e)),
                }
                return;
            }
            reply.error(libc::ENOENT);
            return;
        }
    };

    // Tentar handlers primeiro
    let registry = self.handler_registry.clone();
    let result = self.runtime.block_on(async move {
        registry.handle_read(&path, offset as u64, size as u64).await
    });

    match result {
        Ok(data) => reply.data(&data),
        Err(e) => reply.error(error_to_errno(&e)),
    }
}
```

### 5. Modificar `readdir` para usar Registry

```rust
fn readdir(
    &mut self,
    _req: &Request,
    ino: u64,
    _fh: u64,
    offset: i64,
    mut reply: ReplyDirectory,
) {
    tracing::debug!("FUSE::readdir: ino={}, offset={}", ino, offset);

    let path = if ino == 1 {
        "/".to_string()
    } else {
        match self.path_cache.lock().get(&ino) {
            Some(p) => p.clone(),
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        }
    };

    let registry = self.handler_registry.clone();
    let result = self.runtime.block_on(async move {
        registry.handle_readdir(&path).await
    });

    match result {
        Ok(Some(entries)) => {
            for (i, name) in entries.into_iter().enumerate().skip(offset as usize) {
                let file_type = if name == "." || name == ".." {
                    FileType::Directory
                } else {
                    FileType::RegularFile // Simplificado - usar readdir_plus para tipos corretos
                };

                if reply.add(ino, (i + 1) as i64, file_type, &name) {
                    break;
                }
            }
            reply.ok();
        }
        Ok(None) => reply.error(libc::ENOENT),
        Err(e) => reply.error(error_to_errno(&e)),
    }
}
```

## Exemplo de Uso com GraphDocs

```rust
use crate::handler::{HandlerRegistry, GraphDocsHandler};

fn mount_with_graphdocs(
    fs: Arc<dyn FileSystem>,
    options: &FuseMountOptions,
) -> Result<()> {
    // Criar registry
    let mut registry = HandlerRegistry::with_filesystem(fs.clone());

    // Registrar GraphDocs handler
    registry.register(Arc::new(GraphDocsHandler::new()));

    // Criar FUSE com registry
    let fuse = AgentFSFuse::new(fs, options, Some(Arc::new(registry)));

    // Montar...
    Ok(())
}
```

## Fluxo de Operacao

```
FUSE read("/docs/readme.gd.md", offset=0, size=4096)
    |
    v
AgentFSFuse.read()
    |
    v
handler_registry.handle_read("/docs/readme.gd.md", 0, 4096)
    |
    +---> GraphDocsHandler.can_handle("/docs/readme.gd.md") -> true
    |         |
    |         v
    |     GraphDocsHandler.read() -> Ok(Some(rendered_markdown))
    |         |
    |         v
    +---> Retorna rendered_markdown
    |
    v
reply.data(rendered_markdown)
```

## Consideracoes de Performance

1. **Cache de Handlers**: Avaliar cache de `can_handle()` results
2. **Path Resolution**: Evitar resolver path multiplas vezes
3. **Async Runtime**: Usar `block_on` com cuidado para evitar deadlocks
4. **Handler Priority**: Handlers de alta prioridade devem ser rapidos em `can_handle()`

## Testes Necessarios

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graphdocs_intercept() {
        // Setup registry com GraphDocsHandler
        // Verificar que .gd.md files sao interceptados
    }

    #[test]
    fn test_default_fallback() {
        // Verificar que arquivos normais usam DefaultHandler
    }

    #[test]
    fn test_handler_priority() {
        // Verificar que handlers de maior prioridade sao tentados primeiro
    }
}
```
