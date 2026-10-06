# Source Policies and Range Types

This page explains how S:P:L:I:N:E:D evaluates artwork size, how global Resolution Range policy works, how per-source overrides refine that policy, and how AISPLINE remediation remains separate from provider acceptance.

This policy model applies to the current Python/Docker and Windows releases
using Config v5.

---

## Core model

SPLINED does not simply choose the largest available image.

Candidate evaluation considers:

- effective artwork Resolution Range;
- actual downloaded image dimensions;
- final output geometry/policy;
- source-specific constraints where enabled;
- local artwork state;
- provider metadata/capabilities;
- candidate eligibility and fallback rules.

The normal target is the configured **Ideal** size, not maximum size at any cost.

---

# Global Resolution Range

SPLINED classifies artwork by the **short side** of the image.

Default anchors:

```text
Minimum = 1200
Ideal   = 1800
Maximum = 2400
Ladder  = 3600
```

Default Range Types:

| Range Type | Short side |
| --- | ---: |
| `BelowMinimum` | `< 1200` |
| `LowerRange` | `1200–1799` |
| `Ideal` | `1800` |
| `UpperRange` | `1801–2400` |
| `Ladder` | `2401–3600` |
| `AboveLadder` | `> 3600` |

The short side is used so a very wide/tall image cannot appear high-resolution merely because one dimension is large.

Examples:

```text
900 x 900    -> BelowMinimum
1200 x 1600  -> LowerRange
1800 x 1800  -> Ideal
2000 x 2400  -> UpperRange
3000 x 4200  -> Ladder
4000 x 5000  -> AboveLadder
```

---

# Range Type is the primary policy language

Range Type is the normal user-facing authority for source acceptance.

Advanced numeric fields may add tighter constraints, but they do not replace the Range Type model.

Example:

```text
Minimum Range Type = LowerRange
```

means candidates below `LowerRange` are not normally accepted by that source unless a fallback policy explicitly allows them.

---

# Source Enabled, Strict Override, and Source Override

These are separate controls.

## Source Enabled

Determines whether SPLINED may query the provider.

```text
Enabled = No
```

Provider is not queried.

```text
Enabled = Yes
```

Provider can participate in discovery.

## Source Override

Determines whether a provider uses custom policy instead of the global policy.

```text
Source Override = No
```

The provider follows normal global SPLINED policy.

```text
Source Override = Yes
```

The provider uses its saved source-specific policy.

Turning Source Override off must **not erase** its saved custom values. Those values should remain available if the override is turned back on later.

## Strict Override

`strict_override = true` makes decoded image-content evidence authoritative for
Preferred (`s`) and unattended Auto selection. It supersedes
`source_override`: source-specific range values remain saved, but the global
`[range]` scale is used and scale, resolution, squareness, provider order, and
the image URL cannot independently make the candidate Preferred.

Strict comparison uses the complete decoded image. An exact-release Cover Art
Archive image typed `Front` is authoritative front-cover evidence. A matching
local image may also become an evidence anchor after it matches that Front or
multiple independent front-art sources. Cover Art Archive `Medium`, `Disc`,
`Back`, booklet, tray, and similar types can confirm release identity but do
not validate a front-cover candidate.

Candidates that fail or lack strict evidence remain in Candidate Decision and
their URLs remain previewable. They are excluded only from Preferred and Auto,
so an operator may still select one explicitly. Auto additionally requires an
Ideal, acceptable result with no upscale or crop. This is intentionally more
conservative than interactive Preferred selection.

Python/Ratatui and Windows use the same strict states and eligibility rules.
Windows displays an unverified strict candidate as **Manual only** and exposes
the decoded-content decision in its tooltip and Activity diagnostics.

Amazon defaults to strict mode because Store search can return product photos,
packages, inserts, discs, or other merchandise imagery. Trusted sources can
retain existing behavior with `strict_override = false`. When MusicBrainz
supplies an ASIN for the exact release, Amazon discovery searches that ASIN and
rejects search rows for a different ASIN before decoded-content comparison.

---

# Per-source policy controls

Where supported, a source may have custom values for:

- Source Enabled;
- Strict Content Override;
- Source Override;
- Minimum Range Type;
- Allow BelowMinimum fallback;
- advanced minimum short side;
- advanced maximum short side;
- minimum width;
- minimum height;
- Primary image only.

Not every source exposes metadata needed for every control. Unsupported controls should be disabled or explained rather than pretending to enforce data the provider does not expose.

Python/TUI should expose the same practical Config v5 source controls as Windows where supported. The goal is to eliminate candidates that policy already knows are irrelevant before they consume unnecessary download, AI-review, ranking, and screen space.

---

# Minimum Range Type

When Source Override is enabled, Minimum Range Type sets the normal category floor for that provider.

Example:

```text
Provider: Discogs
Source Override: Yes
Minimum Range Type: LowerRange
```

Conceptual result:

| Candidate | Result |
| --- | --- |
| BelowMinimum | Reject, or fallback if specifically permitted |
| LowerRange | Accept |
| Ideal | Accept |
| UpperRange | Accept |
| Ladder | Accept |
| AboveLadder | Accept unless another constraint rejects it |

A source-specific minimum affects only that provider.

---

# Adjacent-lower fallback

`allow_below_minimum_fallback` is a **per-source** rule. Despite its historical
field name, it admits only the one Range Type immediately below the configured
minimum.

For example, with `Minimum Range Type = LowerRange`, when disabled:

```text
BelowMinimum -> REJECT
```

When enabled:

```text
BelowMinimum -> FALLBACK
```

Fallback is not equivalent to normal acceptance.

It means the candidate can remain available as a last-resort option when no normally acceptable candidate satisfies the effective policy.

With `Minimum Range Type = Ideal`, `LowerRange` is the only fallback;
`BelowMinimum` remains rejected. This prevents the option from becoming an
unbounded relaxation of the source policy.

---

# Advanced dimension constraints

Advanced constraints refine the effective source policy.

Blank values mean no additional restriction beyond the Range Type/global policy.

Example:

```text
Minimum Range Type = LowerRange
```

implies a normal short-side floor of 1200 using the default scale.

If the user also sets:

```text
Minimum short side = 1500
```

then:

| Short side | Result |
| --- | --- |
| `< 1200` | Reject/fallback according to BelowMinimum policy |
| `1200–1499` | Reject due to explicit advanced minimum |
| `1500–1799` | Accept |
| `1800+` | Accept unless another rule rejects it |

The UI should make it clear when a value is derived from Range Type versus explicitly entered by the user.

---

# Width and height constraints

Explicit minimum width/height fields are additional geometry gates.

For example, a candidate may meet the short-side Range Type but still fail an explicit width requirement.

Use these only when a source needs stricter geometry than the global scale provides.

Avoid adding unnecessary advanced constraints if the normal Range Type model already expresses the intended policy.

---

# Primary image only

`Primary image only` relies on **provider metadata**. It is an early metadata
filter and is separate from Strict Override's decoded-content evidence gate.

If the provider exposes a reliable primary/front indicator, the option may use it. If the provider does not expose suitable metadata, the control should be disabled with an explanation.

---

# Early filtering and performance

Source policy should be applied as early as the available metadata permits.

Preferred sequence:

```text
provider discovery
-> provider/source policy
-> geometry/metadata rejection where possible
-> candidate download as required
-> normal SPLINED evaluation/ranking
-> optional AISPLINE review of relevant candidates
-> TUI/source summary
```

Independent provider discovery and candidate downloads may overlap to avoid
adding unrelated network waits together. SPLINED consumes their results in the
configured source and provider-reference order before applying policy and
ranking, so concurrency does not change deterministic priority or selection.

Avoid an architecture that downloads and AI-reviews every returned variant only to hide most of them later.

This optimization must not change normal SPLINED ranking semantics among candidates that remain eligible.

---

# Album authority and curated compilations

Album identity and artwork lookup use tagged Album Artist/Album values
and MusicBrainz Album/Release IDs. Track-level featured performers do not create
extra Artist Picker copies; picker ownership remains the physical top-level
library folder.

A deliberately curated compilation may have `compilation=1`, per-track
MusicBrainz Recording and Artist IDs, and no Album/Release ID. When the selected
Album's representative track has no Album/Release ID and has `compilation=1`,
the unified scan automatically selects the embedded per-track artwork target.
Select Media keeps the yellow warning; there is no separate scan mode.

The compilation branch is explicit and per track:

1. A track with an Album/Release ID remains in the folder-art branch.
2. Valid local Recording and Artist IDs remain the preferred authority. A
   track with missing or unreliable IDs can enter the explicit MusicBrainz
   discovery pane, which searches only its local Artist and Title. The curated
   compilation Album name is never sent as search identity.
3. SPLINED checks its persistent exact Recording-ID/Artist-ID cache.
4. On a cache miss, SPLINED uses the indexed Artist ID to narrow local Albums,
   lazily reads only those tracks, and caches the exact relationships. A
   matching existing Album `cover.*` becomes a `[LOCAL]` candidate.
5. Only after the bounded local lookup misses does SPLINED make a MusicBrainz
   Recording-ID request. It considers the first suitable official release in
   this order: Album, Soundtrack, Compilation. Single, EP, live, remix, DJ-mix,
   and mixtape references are not selected.
6. Normal source discovery and candidate quality policy evaluate artwork for
   the recovered release. The configured `[range].ladder` caps the embedded
   result and no upscaling is introduced.
7. The operator previews and approves the candidate. LIVE WRITE replaces only
   that track's embedded front cover; READ reports what would be replaced.
8. Each successful LIVE WRITE is committed to the per-track ledger
   immediately. A later scan validates the saved Recording/Artist IDs
   and skips that track instead of repeating local or remote discovery.
9. If the ID-first path returns no useful authority, the operator opens the
   MusicBrainz Matches surface (`M` in Ratatui or **MusicBrainz Matches...** on
   Windows). Ratatui shows matches by newest-to-oldest decade (`Unknown` last),
   then populated Album, Single, EP, Compilation, Soundtrack, and additional
   release types. Each row exposes the Cover Art
   Archive release-group front-image URL for Artwork preview, falling back to
   the exact-release endpoint when no group MBID is available. Selecting a
   match makes its Recording/Artist/Release IDs session authority and performs
   the normal local/provider artwork search. The current source-result release
   is green. Previously inspected releases are blue in Ratatui and orange in
   Windows. Re-selecting either restores its
   candidates and diagnostics without repeating provider discovery or image
   downloads. Escape from artwork sources returns to the same cached match
   list. Resolution is populated only after that release has been inspected.
   Preview and Resolution deliberately describe different evidence: preview
   uses release-group authority, while Resolution is cached against the exact
   Release MBID and summarizes the best inspected result from all enabled
   artwork sources. Ratatui Escape or Windows **Back to MB Matches** returns
   from artwork sources to the same cached list. Multiple editions may
   therefore share a preview but retain
   different resolution/source results. Higher resolution is useful comparison
   evidence, not a replacement for normal range, source-priority, shape,
   approval, and fallback policy.
   In Windows, one separate yellow `[CURRENT ALBUM]` category and `[*]`
   authority row remain fixed first and outside Artist/Release-Type filtering.
   The authority row's Release Type cell is blank, so the label appears only in
   the category header. Ordinary rows have no decade grouping: named Artists
   sort A–Z first, followed by Soundtrack, Compilation, and `Various Artists`;
   date, country, Release title, and Release MBID provide the documented
   deterministic ordering within each Artist or special family.
   The filters use ordinary Artist and Release Type column values, intersect,
   and rebuild only populated headings. The list replaces the Scan Activity
   surface while active and uses the existing right-side Artwork panel. Its `[URL]` hover is active
   during MusicBrainz review even when ordinary candidate hover is disabled;
   it is a bounded visual authority check and does not rerun artwork providers.
10. The three `[E]` controls in Ratatui `FALLBACK ARTIST / ALBUM INFO` accept
   session-only MBID corrections. Applying validates the UUID and performs a
   fresh authority/artwork query. Editing Artist or
   Recording invalidates the previously derived Release; editing Release
   performs an exact lookup that must be Official and contain the selected
   Recording and Artist. An exact operator-selected release may be any
   MusicBrainz release type, including a Single or EP; the automatic
   Album, Soundtrack, Compilation restriction in step 5 does not apply.
   Ratatui `M` reopens the active Artist/Title result list. The initial bounded
   search is reused for the active run unless the authority/search identity
   changes. The Windows Artist/Release/Recording fields describe only its
   separate current-Album authority row. **Apply IDs** updates that row and its
   Release-ID page target, validates the edited session authority, and loads
   source results for the exact Release ID. Choosing a candidate may update
   artwork, but never writes the edited IDs to audio tags. Ordinary rows, URLs,
   numbering, selection, filters, and visited state remain unchanged. Windows no longer exposes the separate
   **Search Artist / Track** free-text result branch; automatic compilation
   matching still performs shared Artist/Title lookup when track IDs are absent.

The curated Album name is never used as MusicBrainz identity. SPLINED does not
invent or write an Album/Release ID, write an operator-edited MBID, change the compilation's Album/Artist
tags, or create, alter, or remove folder-level `cover.*`. Different tracks in
one curated compilation may therefore receive different approved artwork.

Timeouts, MusicBrainz `429`/`503` responses, no match, no artwork, rejection,
or a failed write leave the track artwork unchanged and appear in Unresolved.
Positive ID-based Recording-to-release results and lazily inspected local
identities are retained in `splined.db` so later scans avoid repeated
work. Text-discovery choices are cached only for the active run and contain no
credentials or tokens.
Starting LIVE WRITE persists the Album as `incomplete`, including 0/N when
the operator leaves before the first approval, and its picker name is blue.
Ratatui Escape or the corresponding Windows Back/Stop action leaves the current
compilation and returns to the retained Album list; it does not exit SPLINED.
The Album becomes
`processed` only when the verified ledger reaches the full track count.
Retagging a completed track to different Recording/Artist IDs invalidates that
track's resume entry.

Every other Album uses the same Candidate Decision concept with a folder-art
output target. Ratatui `M` or Windows **MusicBrainz Matches...** opens the
integrated release list even when the original Album ID was valid. Selecting an unseen release runs source discovery;
selecting a green current or blue inspected release restores the per-Album
cached source results. Escape from artwork results returns to the cached release
list without rerunning providers.
The automatic acceptable-candidate fast path remains unchanged when no operator
decision is required.

`musicbrainz` is an artwork-policy entry even though MusicBrainz does not host
the bytes: MusicBrainz supplies release authority and Cover Art Archive supplies
the direct image URL. It has independent Minimum Range Type and fallback
settings from exact-release `coverartarchive` discovery. `amazon` searches the
Amazon Store, accepts only primary `m.media-amazon.com/images/I/` artwork, and
removes between-dots resize transforms before preview and evaluation. Amazon is
opt-in because public Store HTML may throttle requests or change markup.

---

# Final-image evaluation

SPLINED should evaluate the image that would actually be written, not blindly trust provider-reported dimensions.

Important reasons include inaccurate provider metadata, output crop/square transforms, output conversion, and downloaded dimensions that differ from advertised dimensions.

Policy principle:

> Candidate ranking should reflect the final image SPLINED would write.

---

# Local artwork

Existing local artwork is evaluated before unnecessary replacement.

A valid local image may be retained if it already satisfies the effective policy and is otherwise authoritative for the current album.

Local artwork is also a first-class potential AISPLINE input when AI integration is enabled. A user should not need to select and install a remote candidate merely to create an image that AISPLINE can review or enhance.

---

# AISPLINE remediation policy

A:I:S:P:L:I:N:E:D is not a retrieval provider.

Normal SPLINED source policies determine whether provider artwork may participate in candidate selection.

When AISPLINE integration is enabled, an already-obtained local or remote candidate may subsequently be evaluated under AISPLINE remediation policy.

Typical baseline relationship:

```text
BelowMinimum 600–1199
    -> high-value remediation candidate

LowerRange 1200–1799
    -> remediation candidate

Ideal 1800
    -> target met
```

AISPLINE does not change the meaning of SPLINED Range Types. It operates after an image candidate exists.

## User floor vs hard lock

The baseline AISPLINE user floor is:

```text
minimum short side = 600 px
```

This is a practical default for normal use, not an absolute quality law. Low-resolution images may be the only available artwork, and some can remain usable despite their dimensions/compression.

Therefore policy may allow deliberate experimentation below the floor. Such attempts must be explicit, user-controlled, and documented as higher-risk/lower-confidence processing. They must never silently weaken the configured floor.

Config v5 may express the baseline as:

```toml
[aisplined]
enabled = true
endpoint = ""
minimum_short_side = 600
allow_below_minimum_override = false
```

No Config v5 version bump is required simply because AISPLINE-specific policy is added inside the existing `[aisplined]` section.

## AISPLINE source-summary semantics

When AI is enabled, AISPLINE review may occur before the candidate summary is presented.

```text
AI SPLINED
    = yes/no result of completed AISPLINE review

AI ENHANCED
    = optional user enhancement action
    = checkbox + required enhancement delta
    = examples: ☐ +300 EH, ☐ +1200 EH, N/A
```

The review result does not mean the image has already been enhanced.

At most one candidate per album may be selected for enhancement. Selecting one `AI ENHANCED` option disables/grays the other enhancement choices for the same album until the selection changes.

A suggested remote candidate can be handed directly to AISPLINE without first being installed into the album directory.

## Previously Enhanced provenance

A previously successful AISPLINE result known through retained history may be presented as an Enhanced candidate only when the corresponding file still exists and validates.

User-facing provenance markers are:

```text
[LOCAL]
[URL]
[Enhanced]
```

`[Enhanced]` is prior-result provenance. It is not the same thing as the current `AI ENHANCED` checkbox.

## `upscale_below_ideal` runtime override

If the user chooses AISPLINE enhancement while ordinary SPLINED output policy has:

```toml
[output]
upscale_below_ideal = false
```

interactive mode should warn clearly and offer a runtime-only override for that one album/candidate/attempt.

The override must not rewrite Config v5, affect later albums, or silently enable ordinary SPLINED upscaling.

---

# Candidate status language

The GUI/TUI may show candidate results such as:

```text
lower-range - Acceptable
below-minimum - Fallback
```

These labels describe the candidate's effective range/policy result. They should not be confused with Select Media/tree status colors, which represent album/artist execution state rather than image quality.

---

# Troubleshooting source policy

If a candidate you expected to see is missing:

1. confirm the source is Enabled;
2. confirm Strict Override and inspect `strict.content` log results;
3. confirm Source Override state;
4. check Minimum Range Type;
5. check adjacent-lower fallback;
6. inspect advanced short-side/width/height limits;
7. check Primary image only if supported;
8. review the scan log for policy-filtered candidate counts;
9. verify the provider actually returned the candidate;
10. verify the downloaded image dimensions.

If the scan log reports candidates hidden by active source policy, discovery succeeded but effective policy filtered those candidates from the review set.

---

# Relationship to Config v5

For the broader configuration model, see [Config v5 reference](config-v5-reference.md).

Exact serialized Config v5 keys/defaults are listed in the Config v5 reference and examples.

---

# Related documentation

- [Config v5 reference](config-v5-reference.md)
- [Windows guide](windows-guide.md)
- [Credentials and provider setup](credentials-providers.md)
- [Select Media and status colors](media-filter-status-colors.md)
- [Python Ratatui TUI](ratatui-tui.md)
