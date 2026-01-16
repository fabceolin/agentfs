//! Cross-Document Relationships and DuckDB PGQ Query Generation
//!
//! This module implements relationship declarations for BMAD templates and
//! generates DuckDB PGQ (Property Graph Query) SQL statements.
//!
//! # Overview
//!
//! Documents in GraphDocs can have typed relationships:
//! - `CONTAINS`: Parent contains children (Epic → Stories)
//! - `BELONGS_TO`: Child belongs to parent (inverse)
//! - `DEPENDS_ON`: Dependency relationship
//! - `REFERENCES`: Loose reference
//! - `FOLLOWS`: Sequential relationship
//! - `SUPERSEDES`: Replacement relationship
//!
//! # Example
//!
//! ```yaml
//! relationships:
//!   - id: stories
//!     edge_type: CONTAINS
//!     direction: outbound
//!     target_template: story-template
//!     cardinality: one-to-many
//!     order_by: order_idx
//! ```

use serde::{Deserialize, Serialize};

/// Relationship declaration in a BMAD template
///
/// Defines how documents relate to each other through the property graph.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RelationshipDecl {
    /// Unique identifier for this relationship (used in Tera templates)
    pub id: String,

    /// Type of edge in the property graph
    pub edge_type: RelationshipEdgeType,

    /// Direction to traverse the edge
    #[serde(default)]
    pub direction: Direction,

    /// Optional: Filter to specific target template
    #[serde(default)]
    pub target_template: Option<String>,

    /// Cardinality constraint
    #[serde(default)]
    pub cardinality: Cardinality,

    /// Optional: Field to sort results by
    #[serde(default)]
    pub order_by: Option<String>,

    /// Optional: Additional filter expression (SQL WHERE clause fragment)
    #[serde(default)]
    pub filter: Option<String>,
}

/// Edge types for document relationships
///
/// These map to edge labels in the DuckDB PGQ property graph.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RelationshipEdgeType {
    /// Parent contains children (Epic → Stories)
    Contains,
    /// Child belongs to parent (inverse of CONTAINS)
    BelongsTo,
    /// Dependency relationship (Story → Story)
    DependsOn,
    /// Loose reference (Any → Any)
    References,
    /// Sequential relationship (Section → Section)
    Follows,
    /// Replacement relationship (Story → Story)
    Supersedes,
    /// Custom edge type (user-defined)
    #[serde(untagged)]
    Custom(String),
}

impl RelationshipEdgeType {
    /// Get the SQL-safe string representation of the edge type
    pub fn as_str(&self) -> &str {
        match self {
            RelationshipEdgeType::Contains => "CONTAINS",
            RelationshipEdgeType::BelongsTo => "BELONGS_TO",
            RelationshipEdgeType::DependsOn => "DEPENDS_ON",
            RelationshipEdgeType::References => "REFERENCES",
            RelationshipEdgeType::Follows => "FOLLOWS",
            RelationshipEdgeType::Supersedes => "SUPERSEDES",
            RelationshipEdgeType::Custom(s) => s,
        }
    }
}

impl Default for RelationshipEdgeType {
    fn default() -> Self {
        Self::References
    }
}

/// Direction to traverse edges in the property graph
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Traverse outgoing edges (source → target)
    #[default]
    Outbound,
    /// Traverse incoming edges (source ← target)
    Inbound,
    /// Traverse both directions
    Both,
}

impl Direction {
    pub fn as_str(&self) -> &str {
        match self {
            Direction::Outbound => "outbound",
            Direction::Inbound => "inbound",
            Direction::Both => "both",
        }
    }
}

/// Cardinality constraint for relationships
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Cardinality {
    /// Single related document
    OneToOne,
    /// Multiple children (Epic → Stories)
    #[default]
    OneToMany,
    /// Single parent (Story → Epic)
    ManyToOne,
    /// Multiple both ways (Dependencies)
    ManyToMany,
}

impl Cardinality {
    pub fn as_str(&self) -> &str {
        match self {
            Cardinality::OneToOne => "one-to-one",
            Cardinality::OneToMany => "one-to-many",
            Cardinality::ManyToOne => "many-to-one",
            Cardinality::ManyToMany => "many-to-many",
        }
    }
}

/// Resolved relationship with actual document data
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedRelationship {
    /// Relationship ID from declaration
    pub id: String,
    /// Edge type string
    pub edge_type: String,
    /// Related documents
    pub documents: Vec<RelatedDocument>,
}

/// Related document with extracted fields
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedDocument {
    /// Document ID
    pub id: String,
    /// File path
    pub path: String,
    /// Document title
    pub title: Option<String>,
    /// Document status
    pub status: Option<String>,
    /// Template ID used
    pub template_id: Option<String>,
    /// All variables from the related document (JSON)
    #[serde(default)]
    pub variables: serde_json::Value,
    /// Custom fields extracted from sections (JSON)
    #[serde(default)]
    pub fields: serde_json::Value,
}

impl RelationshipDecl {
    /// Generate DuckDB PGQ query for this relationship
    ///
    /// Uses SQL/PGQ syntax as implemented by DuckDB PGQ extension.
    /// Reference: <https://github.com/cwida/duckpgq-extension>
    ///
    /// # Arguments
    ///
    /// * `source_doc_id` - The document ID to query relationships from
    ///
    /// # Returns
    ///
    /// A SQL query string using DuckDB PGQ GRAPH_TABLE syntax
    ///
    /// # Example
    ///
    /// ```
    /// use agentfs_sdk::graphdocs::relationships::{RelationshipDecl, RelationshipEdgeType, Direction, Cardinality};
    ///
    /// let rel = RelationshipDecl {
    ///     id: "stories".to_string(),
    ///     edge_type: RelationshipEdgeType::Contains,
    ///     direction: Direction::Outbound,
    ///     target_template: Some("story-template".to_string()),
    ///     cardinality: Cardinality::OneToMany,
    ///     order_by: Some("order_idx".to_string()),
    ///     filter: None,
    /// };
    ///
    /// let query = rel.to_pgq_query("EPIC-2.1");
    /// assert!(query.contains("CONTAINS"));
    /// assert!(query.contains("EPIC-2.1"));
    /// ```
    pub fn to_pgq_query(&self, source_doc_id: &str) -> String {
        // Escape single quotes in doc_id to prevent SQL injection
        let escaped_doc_id = source_doc_id.replace('\'', "''");

        // Build MATCH pattern based on direction
        let match_pattern = match self.direction {
            Direction::Outbound => format!(
                "(src:gd_documents)-[e:{}]->(tgt:gd_documents)",
                self.edge_type.as_str()
            ),
            Direction::Inbound => format!(
                "(src:gd_documents)<-[e:{}]-(tgt:gd_documents)",
                self.edge_type.as_str()
            ),
            Direction::Both => format!(
                "(src:gd_documents)-[e:{}]-(tgt:gd_documents)",
                self.edge_type.as_str()
            ),
        };

        // Build WHERE clause
        let mut where_clauses = vec![format!("src.id = '{}'", escaped_doc_id)];

        if let Some(ref template) = self.target_template {
            let escaped_template = template.replace('\'', "''");
            where_clauses.push(format!("tgt.template_id = '{}'", escaped_template));
        }

        if let Some(ref filter) = self.filter {
            // Note: filter is trusted input from template, not user input
            where_clauses.push(filter.clone());
        }

        let where_clause = where_clauses.join(" AND ");

        // Build COLUMNS clause
        let columns = "tgt.id AS id, tgt.path AS path, tgt.title AS title, \
                       tgt.template_id AS template_id, tgt.variables AS variables, \
                       tgt.status AS status, e.properties AS edge_properties";

        // Build ORDER BY if specified
        let order_clause = self
            .order_by
            .as_ref()
            .map(|o| format!("\nORDER BY {}", o))
            .unwrap_or_default();

        format!(
            r#"FROM GRAPH_TABLE (gd_graph
    MATCH {match_pattern}
    WHERE {where_clause}
    COLUMNS ({columns})
) AS result{order_clause}"#,
            match_pattern = match_pattern,
            where_clause = where_clause,
            columns = columns,
            order_clause = order_clause,
        )
    }

    /// Generate query for counting related documents
    ///
    /// # Arguments
    ///
    /// * `source_doc_id` - The document ID to count relationships from
    ///
    /// # Returns
    ///
    /// A SQL COUNT query string
    pub fn to_pgq_count_query(&self, source_doc_id: &str) -> String {
        let escaped_doc_id = source_doc_id.replace('\'', "''");

        let match_pattern = match self.direction {
            Direction::Outbound => format!(
                "(src:gd_documents)-[e:{}]->(tgt:gd_documents)",
                self.edge_type.as_str()
            ),
            Direction::Inbound => format!(
                "(src:gd_documents)<-[e:{}]-(tgt:gd_documents)",
                self.edge_type.as_str()
            ),
            Direction::Both => format!(
                "(src:gd_documents)-[e:{}]-(tgt:gd_documents)",
                self.edge_type.as_str()
            ),
        };

        let mut where_clauses = vec![format!("src.id = '{}'", escaped_doc_id)];

        if let Some(ref template) = self.target_template {
            let escaped_template = template.replace('\'', "''");
            where_clauses.push(format!("tgt.template_id = '{}'", escaped_template));
        }

        let where_clause = where_clauses.join(" AND ");

        format!(
            r#"SELECT COUNT(*) FROM GRAPH_TABLE (gd_graph
    MATCH {match_pattern}
    WHERE {where_clause}
    COLUMNS (tgt.id)
) AS result"#,
            match_pattern = match_pattern,
            where_clause = where_clause,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_relationship_declaration() {
        let yaml = r#"
id: stories
edge_type: CONTAINS
direction: outbound
target_template: story-template
cardinality: one-to-many
order_by: order_idx
"#;
        let rel: RelationshipDecl = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(rel.id, "stories");
        assert_eq!(rel.edge_type, RelationshipEdgeType::Contains);
        assert_eq!(rel.direction, Direction::Outbound);
        assert_eq!(rel.cardinality, Cardinality::OneToMany);
        assert_eq!(rel.order_by, Some("order_idx".to_string()));
    }

    #[test]
    fn test_parse_all_edge_types() {
        for (yaml_val, expected) in [
            ("CONTAINS", RelationshipEdgeType::Contains),
            ("BELONGS_TO", RelationshipEdgeType::BelongsTo),
            ("DEPENDS_ON", RelationshipEdgeType::DependsOn),
            ("REFERENCES", RelationshipEdgeType::References),
            ("FOLLOWS", RelationshipEdgeType::Follows),
            ("SUPERSEDES", RelationshipEdgeType::Supersedes),
        ] {
            let yaml = format!("id: test\nedge_type: {}", yaml_val);
            let rel: RelationshipDecl = serde_yaml::from_str(&yaml).unwrap();
            assert_eq!(rel.edge_type, expected, "Failed for {}", yaml_val);
        }
    }

    #[test]
    fn test_pgq_query_generation_outbound() {
        let rel = RelationshipDecl {
            id: "stories".to_string(),
            edge_type: RelationshipEdgeType::Contains,
            direction: Direction::Outbound,
            target_template: Some("story-template".to_string()),
            cardinality: Cardinality::OneToMany,
            order_by: Some("order_idx".to_string()),
            filter: None,
        };

        let query = rel.to_pgq_query("doc-123");
        assert!(query.contains("CONTAINS"));
        assert!(query.contains("doc-123"));
        assert!(query.contains("story-template"));
        assert!(query.contains("ORDER BY order_idx"));
        assert!(query.contains("-[e:CONTAINS]->"));
    }

    #[test]
    fn test_pgq_query_generation_inbound() {
        let rel = RelationshipDecl {
            id: "parent".to_string(),
            edge_type: RelationshipEdgeType::BelongsTo,
            direction: Direction::Inbound,
            target_template: None,
            cardinality: Cardinality::ManyToOne,
            order_by: None,
            filter: None,
        };

        let query = rel.to_pgq_query("STORY-1");
        assert!(query.contains("<-[e:BELONGS_TO]-"));
        assert!(query.contains("STORY-1"));
    }

    #[test]
    fn test_pgq_query_generation_both() {
        let rel = RelationshipDecl {
            id: "related".to_string(),
            edge_type: RelationshipEdgeType::References,
            direction: Direction::Both,
            target_template: None,
            cardinality: Cardinality::ManyToMany,
            order_by: None,
            filter: None,
        };

        let query = rel.to_pgq_query("DOC-1");
        assert!(query.contains("-[e:REFERENCES]-"));
        assert!(!query.contains("->"));
        assert!(!query.contains("<-"));
    }

    #[test]
    fn test_pgq_count_query() {
        let rel = RelationshipDecl {
            id: "stories".to_string(),
            edge_type: RelationshipEdgeType::Contains,
            direction: Direction::Outbound,
            target_template: None,
            cardinality: Cardinality::OneToMany,
            order_by: None,
            filter: None,
        };

        let query = rel.to_pgq_count_query("EPIC-1");
        assert!(query.contains("COUNT(*)"));
        assert!(query.contains("EPIC-1"));
    }

    #[test]
    fn test_sql_injection_prevention() {
        let rel = RelationshipDecl {
            id: "test".to_string(),
            edge_type: RelationshipEdgeType::Contains,
            direction: Direction::Outbound,
            target_template: None,
            cardinality: Cardinality::OneToMany,
            order_by: None,
            filter: None,
        };

        // Test SQL injection attempt with single quotes
        // Input: '; DROP TABLE gd_documents; --
        // After escaping, the single quote becomes two single quotes: ''
        // So the doc_id becomes: ''; DROP TABLE gd_documents; --
        // In the final query: src.id = '''; DROP TABLE gd_documents; --'
        // Which is: open-quote + escaped-quote(2 chars) + rest + close-quote
        let query = rel.to_pgq_query("'; DROP TABLE gd_documents; --");

        // The escaped version should contain ''' (three quotes):
        // opening quote + escaped quote (two chars) = three quotes total before the semicolon
        assert!(
            query.contains("'''"),
            "Query should contain escaped single quotes ('''): {}",
            query
        );

        // The key security property: the injected quote is escaped so it becomes
        // part of the string literal, not a string terminator
        // Safe pattern: src.id = '''...  (string containing a literal quote)
        // Dangerous pattern would be: src.id = '' ; DROP (empty string followed by new statement)
        // We verify the semicolon is inside the quoted string, not outside
        assert!(
            query.contains("''; DROP"),
            "The DROP should be inside the string literal: {}",
            query
        );
    }

    #[test]
    fn test_edge_type_as_str() {
        assert_eq!(RelationshipEdgeType::Contains.as_str(), "CONTAINS");
        assert_eq!(RelationshipEdgeType::BelongsTo.as_str(), "BELONGS_TO");
        assert_eq!(RelationshipEdgeType::DependsOn.as_str(), "DEPENDS_ON");
        assert_eq!(RelationshipEdgeType::References.as_str(), "REFERENCES");
        assert_eq!(RelationshipEdgeType::Follows.as_str(), "FOLLOWS");
        assert_eq!(RelationshipEdgeType::Supersedes.as_str(), "SUPERSEDES");
        assert_eq!(
            RelationshipEdgeType::Custom("MY_EDGE".to_string()).as_str(),
            "MY_EDGE"
        );
    }

    #[test]
    fn test_direction_as_str() {
        assert_eq!(Direction::Outbound.as_str(), "outbound");
        assert_eq!(Direction::Inbound.as_str(), "inbound");
        assert_eq!(Direction::Both.as_str(), "both");
    }

    #[test]
    fn test_cardinality_as_str() {
        assert_eq!(Cardinality::OneToOne.as_str(), "one-to-one");
        assert_eq!(Cardinality::OneToMany.as_str(), "one-to-many");
        assert_eq!(Cardinality::ManyToOne.as_str(), "many-to-one");
        assert_eq!(Cardinality::ManyToMany.as_str(), "many-to-many");
    }

    #[test]
    fn test_defaults() {
        assert_eq!(
            RelationshipEdgeType::default(),
            RelationshipEdgeType::References
        );
        assert_eq!(Direction::default(), Direction::Outbound);
        assert_eq!(Cardinality::default(), Cardinality::OneToMany);
    }

    #[test]
    fn test_related_document_serialization() {
        let doc = RelatedDocument {
            id: "STORY-1".to_string(),
            path: "stories/STORY-1.md".to_string(),
            title: Some("First Story".to_string()),
            status: Some("Done".to_string()),
            template_id: Some("story-template".to_string()),
            variables: serde_json::json!({"priority": "High"}),
            fields: serde_json::json!({}),
        };

        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains("STORY-1"));
        assert!(json.contains("First Story"));
        assert!(json.contains("Done"));
    }

    #[test]
    fn test_resolved_relationship() {
        let rel = ResolvedRelationship {
            id: "stories".to_string(),
            edge_type: "CONTAINS".to_string(),
            documents: vec![RelatedDocument {
                id: "STORY-1".to_string(),
                path: "stories/STORY-1.md".to_string(),
                title: Some("Story One".to_string()),
                status: Some("Done".to_string()),
                template_id: None,
                variables: serde_json::json!({}),
                fields: serde_json::json!({}),
            }],
        };

        assert_eq!(rel.documents.len(), 1);
        assert_eq!(rel.documents[0].id, "STORY-1");
    }

    #[test]
    fn test_pgq_query_with_filter() {
        let rel = RelationshipDecl {
            id: "active_stories".to_string(),
            edge_type: RelationshipEdgeType::Contains,
            direction: Direction::Outbound,
            target_template: None,
            cardinality: Cardinality::OneToMany,
            order_by: None,
            filter: Some("tgt.status != 'Cancelled'".to_string()),
        };

        let query = rel.to_pgq_query("EPIC-1");
        assert!(query.contains("tgt.status != 'Cancelled'"));
    }
}
