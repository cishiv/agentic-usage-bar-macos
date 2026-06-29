# Specifications Workflow

Specs live in two subdirectories:

- `NOT_YET_IMPLEMENTED/` — specs that describe planned, unbuilt features.
- `IMPLEMENTED/` — specs whose features have shipped.

## Naming

- Not implemented: `FEATURE_NAME_{YYYYDDMM}.md`
- Implemented: `IMPLEMENTED_FEATURE_NAME_{YYYYDDMM}.md`

## Lifecycle

1. Write the spec in `NOT_YET_IMPLEMENTED/` and commit it.
2. Implement the feature.
3. Rename the spec to add the `IMPLEMENTED_` prefix, move it into
   `IMPLEMENTED/`, and commit that move **in its own separate commit**:
   `git commit -m "spec: mark FEATURE_NAME as implemented"`.

The spec-move commit must be separate from the code commits.
