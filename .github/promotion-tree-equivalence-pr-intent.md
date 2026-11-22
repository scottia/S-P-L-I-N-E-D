# PR intent: promotion tree-equivalence provenance

Correct the exact `dev` to `main` promotion provenance check for the repository's long-lived branch topology. The check will require current branch heads and prove that combining current `main` with current `dev` produces exactly the `dev` tree, without conflicts or omitted `main` content.

No application behavior, protected policy JSON, release metadata, or protected branch is changed by this intent commit.
