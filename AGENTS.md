# SPLINED repository instructions

These instructions apply to the entire repository.

## Mandatory policy review

Before auditing, changing, building, testing, documenting, committing, or
preparing a pull request, read the complete policy register in this file.

- Treat the user's current wording in the policy register as the authoritative
  definition of SPLINED behavior.
- Map every requested behavior change to the applicable policy entry before
  editing source.
- If the request conflicts with the policy, or the relevant behavior is absent,
  blank, or ambiguous, identify the conflict or gap and obtain the user's
  direction before changing source.
- Do not infer a policy change from existing code, tests, database contents,
  release behavior, commit history, or pull requests. Those are implementation
  evidence and may reveal a mismatch, but they do not silently override policy.
- Do not edit the policy register unless the user explicitly asks for a policy
  change.
- Never place credentials, API keys, passwords, credential hashes, private
  configuration values, or machine-specific test paths in the policy,
  repository changes, commits, pull requests, or test reports.

## Change validation

Before presenting a completed change:

1. compare the implementation with every affected policy entry;
2. run checks appropriate to the changed behavior;
3. report the policy entries reviewed, tests performed, results, and any
   unresolved difference; and
4. do not claim WebGUI and Windows parity unless the policy and observed tests
   both support that claim.

Keep work local for review unless the user explicitly authorizes committing,
pushing, opening or updating a pull request, or merging.

## Authoritative policy register

The user owns policy wording and approval. Source code, tests, releases,
database contents, commits, and historical pull requests provide implementation
evidence; they do not independently redefine policy.

- `WEBGui` and `Windows` describe the required behavior for each architecture.
- `Varied` accepts `current` when no policy difference exists or `changed` when
  a reviewed difference or change exists.
- `Change summary` records only the concise, relevant reason and review or pull
  request reference for a `changed` entry.
- `Comment/Change` is the editable working column for the user's corrections,
  decisions, and proposed policy revisions.
- Blank or ambiguous entries are not permission to infer behavior. Obtain the
  user's direction before implementing behavior not governed clearly here.

| WEBGui | Windows | Varied | Change summary | Comment/Change |
| --- | --- | --- | --- | --- |
|  |  |  |  |  |
