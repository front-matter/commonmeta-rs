# commonmeta-rs — 4-Day Spike Plan (Based on `commonmeta` Go implementation)

## Purpose

This document describes a **4-day time-boxed spike** to start the Rust implementation of Commonmeta (`commonmeta-rs`), explicitly **based on the existing Go implementation (`commonmeta`)**.

The spike is exploratory and evaluative. Its purpose is **not** feature completeness or production readiness, but to:

- Validate that Go `commonmeta` semantics can be restated cleanly in Rust
- Identify where Rust provides clear benefits (and where it does not)
- De-risk the architectural approach before committing to a full Stage-1 implementation

## Non-goals (Important)

This spike explicitly does **not** aim to:

- Achieve feature parity with Go or Python
- Replace or compete with the Go CLI
- Integrate Python bindings
- Optimize performance
- Finalize schemas or APIs

Anything outside the narrow DOI → Crossref → Commonmeta path is out of scope.

## High-level spike goals

By the end of Day 4, we should be able to confidently answer:

1. Can Go `commonmeta` semantics be expressed more explicitly and safely in Rust?
2. Does Rust meaningfully improve correctness and clarity for core logic?
3. What parts of Go logic map cleanly to Rust types, and what needs redesign?
4. Is proceeding to a full Stage-1 implementation justified?

## Scope (Frozen for the spike)

**Input**
- DOI (string)

**Source**
- Crossref only

**Output**
- Canonical Commonmeta JSON (as produced by Go)

## Day-by-day plan

### Day 1 — Understand and freeze Go semantics

**Goal:** Develop a precise understanding of the behavioral semantics of Go `commonmeta`, independent of implementation details.

**Action items:**
- Identify the minimal execution path in Go
- Document normalization, mapping, and error semantics
- Select representative DOIs and capture Go outputs

**Deliverables:**
- `docs/go-semantics.md`
- Saved Go JSON outputs

### Day 2 — Rust skeleton, errors, and identifier model

**Goal:** Establish Rust foundations for encoding Go intent.

**Action items:**
- Create crate structure
- Define error model
- Implement DOI parsing and normalization
- Write unit tests

**Deliverables:**
- Compiling crate
- Typed errors
- DOI model and tests

### Day 3 — Crossref resolution and minimal Commonmeta model

**Goal:** Prove DOI → Crossref → Commonmeta works in Rust.

**Action items:**
- Implement Crossref fetch
- Define minimal Commonmeta record
- Compare Rust and Go outputs

**Deliverables:**
- Working resolver
- Comparable JSON output

### Day 4 — Diffing, reflection, and decision

**Goal:** Decide whether and how to proceed to Stage 1.

**Action items:**
- Diff Go vs Rust outputs
- Document findings
- Make go/no-go recommendation

**Deliverables:**
- `docs/spike-findings.md`
- Stage-1 recommendation

## Guiding principle

> This spike succeeds if Rust makes Go’s behavior **more explicit and understandable**, not faster or more feature-rich.
