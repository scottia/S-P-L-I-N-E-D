# PR intent: Workflow 05 synchronized-tree precondition

Correct Workflow 05's source-preparation precondition for the repository's merge-commit promotion topology. The workflow will require current `main` and `dev` to contain the exact same tree rather than the same commit ID before preparing a source version.

No source version, application behavior, protected policy JSON, release metadata, or protected branch is changed by this intent commit.
