# Story 99.1: Test Conformance Feature

## Status

Draft

## Story

**As a** developer,
**I want** to test the conformance pipeline with different LLM providers,
**so that** I can verify the TEA integration works correctly.

## Description

This is a test story to verify that the conformance pipeline correctly:
- Detects non-conformant documents
- Transforms them using TEA with the specified LLM provider
- Writes the .conformant file with the transformed content

The story should be missing the "Acceptance Criteria" and "Tasks / Subtasks" sections
which are required by the story template. The LLM should add placeholders for these.

## Notes

- Testing with Claude shell provider
- Testing with Gemma3n:e4b provider
- Verifying background conformance works

## Change Log

| Date | Version | Description | Author |
|------|---------|-------------|--------|
| 2026-01-18 | 0.1 | Initial test document | QA |
