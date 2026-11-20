# Artist COMP report policy

The canonical checked-in seed is [`schema/comp-report-policy.json`](../schema/comp-report-policy.json), protected by the repository review requirement in [POLICY-REGISTER.md](POLICY-REGISTER.md). Deliberate, validated edits to an installed policy revision through Settings do not change this checked-in source. Workflow 01 validates its source contract before release version stamping; this is not a live policy-mirror audit.

`comp-report-policy.json` is the shared, editable contract for Artist COMP
identity recovery, MusicBrainz result grouping, existing-artwork reuse, batch
completion, and cross-client publication. WebGUI stores the authoritative
revision in PostgreSQL. Coordinated Windows starts from a validated local
mirror so an unavailable PostgreSQL server cannot delay application startup.

The policy contains workflow rules only. Track identity, artwork digests,
completion evidence, and opposing-writer state remain database records.
Passwords and database URLs belong in `album-coordination.json`, never this
policy.

## Supported installations

- WebGUI seeds the singleton PostgreSQL policy from
  `schema/comp-report-policy.json` during migration.
- Standalone Windows SQLite uses the repository default.
- Shared-SQLite-only Windows creates or adopts an editable policy in the
  selected Provider Credential Directory.
- Coordinated Windows creates a validated local mirror only after the WebGUI
  PostgreSQL connection and both shared policy tables validate.

Existing files are validated and adopted; Setup does not replace a different
existing policy. With **Advanced** enabled, edit the policy under **Settings →
Connections → Artist COMP Report Policy · JSON**. A coordinated Windows save
writes PostgreSQL first and mirrors the returned revision locally. WebGUI saves
the same revisioned PostgreSQL authority.

## Deterministic synchronization and batch pinning

Windows never contacts PostgreSQL on its blocking startup path. It:

1. loads its last validated local policy mirror and opens normally;
2. checks PostgreSQL for both shared policy revisions on a background thread;
3. continues using the local mirrors if that bounded check fails;
4. waits until no scan, artwork write, or COMP operation is active before
   activating a newer validated revision; and
5. captures one immutable policy snapshot when a COMP batch starts.

WebGUI stores that same policy snapshot and revision in every Album request
created by one Artist **LAUNCH COMP** action. All requests from that launch
therefore use the same rules even if an administrator saves a new policy while
the queue is still running. A direct single-track action pins the active
revision for that action.

No client reloads policy between tracks in one batch. The next idle operation
uses the newly activated revision.

## Artist identity and aliases

`artist_identity` owns:

- `credit_separators` for multi-Artist credits such as `Artist A/Artist B`,
  semicolon, `feat.`, `featuring`, `with`, and `vs.`;
- normalization of Unicode, case, whitespace, leading symbols such as `*`, the
  leading article `the`, and the `&`/`and` equivalence;
- selected-Artist precedence, so a report opened for `*NSYNC` keeps `*NSYNC`
  focused for a track credited `*NSYNC/Gloria Estefan`;
- local name/alias similarity thresholds; and
- bounded MusicBrainz Artist-name recovery when embedded Artist IDs are absent,
  invalid, or point to another Artist.

A stale or missing embedded Artist ID is not an immediate terminal failure.
SPLINED first searches bounded MusicBrainz Artist-name results, applies both the
provider threshold and local normalized-name threshold, then constrains all
Recording searches to the recovered Artist ID.

## Recording search and displayed results

`recording_search` defines the minimum fuzzy score, exact and related-Work
scores, maximum score for a non-exact Recording, result limit, and bounded
detail lookups. `result_grouping` defines descending percentage bands and the
display order of Album, Single, EP, Soundtrack, Compilation, Broadcast, and
Other official release families.

An exact tagged Recording remains authoritative. Same-Artist title and related
Work results remain visibly lower-ranked alternatives and do not silently
replace the tagged Recording identity.

## Existing artwork, completion, and publication

Existing embedded artwork is displayed before remote candidates and remains a
valid reuse choice while identity is pending. Reusing its pixels does not mark
the track authoritative until required Artist and Recording identity is valid.

Each requested track ends as exactly one batch classification:

- `completed` — verified artwork and authoritative completion evidence;
- `unresolved` — usable verified artwork may exist, but authoritative identity
  is still incomplete; or
- `failed` — the requested write or verification did not complete.

After every track, the visible report refreshes from durable state. A verified
artwork write publishes its coordination event immediately, including an
unresolved-identity write when the policy permits it. Only authoritative IDs
publish the completion record that makes a COMP track green.

## Editing and backup

Keep `schema_version` and `policy_id` unchanged. Invalid thresholds, missing
completion states, incomplete release-family ordering, disabled batch pinning,
or unsafe synchronization rules are rejected without activation.

WebGUI **Back Up Safe Settings JSON** and **Download Shared Settings Contract**
include both `media-state-classification.json` and `comp-report-policy.json`.
Provider credentials and PostgreSQL data remain excluded.
