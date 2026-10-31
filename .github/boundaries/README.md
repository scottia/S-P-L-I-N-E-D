# Local change-boundary checker

This directory contains the reviewed scope policy, the local checker, and only
its isolated unit tests. It does not alter application behavior or select a
release. Repository enforcement is **not active** until a later, separately
approved workflow and required repository check invoke the trusted checker.

## Decisions kept separate

The checker performs three independent operations:

1. classify every changed path and protected version-field change;
2. compare those categories with explicit scope authorizations;
3. select the validation lanes implied by the classified changes.

A rejected change still reports its required validation lanes. Passing tests
cannot authorize a path, and an authorization does not suppress validation.

The categories are `windows`, `ubuntu-webgui`, `shared-contract`,
`release-version`, `terminology-docs`, `root-native-other`, and
`boundary-policy`. Unknown paths fail closed. Renames or copies whose old and
new paths have different categories fail even when both categories are
authorized.

## Trusted policy and self-modification

Normal checks load both checker policy and authorization rules from the trusted
base revision, not from the proposed head. A future required workflow must
fetch the pull request base, copy this directory from that base to a temporary
location, and execute that trusted copy against `base...head`.

Any change to `.github/boundaries/**` is `boundary-policy`. Such a change must
be isolated: including an application, documentation, shared-contract, or
release file in the same change is rejected even if every category is passed
on the command line. This prevents a policy modification from authorizing an
unrelated edit in the same proposal.

The single future path `.github/workflows/boundary.yml` is classified as
`shared-contract`, following the existing CI/workflow convention. That exact
classification is not approval to implement or activate the workflow. It
requires explicit `shared-contract` authorization and selects Windows,
Ubuntu-WebGUI, and root-native validation; neither `windows` nor
`ubuntu-webgui` authorization permits it. No other new workflow path is
classified by this addition.

That future workflow is enforcement infrastructure. Its implementation needs
an isolated review and a trusted bootstrap into the base branch before it can
evaluate later proposals. The proposed workflow cannot approve its own
introduction, and later integration must treat it as protected enforcement
infrastructure rather than ordinary application code.

This initial policy is a bootstrap case because the recorded baseline does not
contain it. Local bootstrap review may pass `--policy-file`, but the bootstrap
change must contain only `.github/boundaries/**`. After the policy is accepted,
subsequent checks must omit `--policy-file` and load the policy from `--base`.

## Supplying authorization

For local review, authorization is explicit and repeatable:

```text
python .github/boundaries/check_boundary.py --base <base-sha> --head <head-sha> --authorize windows
```

Approved shared work uses `--authorize shared-contract`; the policy then
selects Windows, Ubuntu-WebGUI, and root-native contract validation without
granting either application lane permission to edit the other. A deliberately
cross-platform functional change must receive both `--authorize windows` and
`--authorize ubuntu-webgui`. Protected release fields additionally require
`--authorize release-version`.

When repository enforcement is later proposed, authorizations should come from
maintainer-applied, protected pull-request labels such as `scope:windows` and
`scope:shared-contract`. The future workflow should translate those labels to
`--authorize` arguments. Authorization must not come from a file changed by the
pull request, a commit message, or other contributor-controlled text.

## Protected version fields

The checker compares trusted-base and proposed-head values for root and Windows
Cargo package versions, their SPLINED lockfile entries, Windows executable and
package manifests, the App Installer template, and WebGUI `VERSION` and
`USER_AGENT` in `webgui/backend/splined.py`. Changing one adds
`release-version` scope without reclassifying ordinary functional edits to the
same source file.

## Isolated checker tests

Run only the checker tests with bytecode generation disabled:

```text
PYTHONDONTWRITEBYTECODE=1 python -m unittest discover -s .github/boundaries -p "test_*.py" -v
```

On PowerShell, set `$env:PYTHONDONTWRITEBYTECODE = "1"` for that process.
