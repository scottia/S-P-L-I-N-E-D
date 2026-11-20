# Artwork source policies and Resolution Range Types

This technical reference describes the candidate acceptance controls
implemented through Config v5, the Rust source-policy modules, and
WebGUI's provider filtering. It is **not** a new owner-approved
Artist/Album state policy and must never supersede
[POLICY-REGISTER.md](POLICY-REGISTER.md).

## Candidate quality model

SPLINED does not always select the largest source image. It
considers downloaded/decoded dimensions, available front-art
evidence, selected source, configured resolution bands, actual
output geometry and user-approved enhancement/embedding operations.

| Term | Meaning |
| --- | --- |
| **Resolution Range** | Shared quality/range ladder for candidate size and preferred output classification |
| **Minimum Range Type** | Normal quality floor for a provider when custom source policy is enabled |
| **Ideal / Preferred** | Target quality bands; a candidate below Ideal may still be usable within its permitted range |
| **Source Enabled** | Whether a provider participates in applicable artwork search |
| **Source Override** | Whether saved provider-specific range/dimension rules replace the ordinary global quality choices |
| **Strict Override** | Stronger decoded image/content evidence gate, independent of whether older Source Override settings are retained |
| **Adjacent-lower fallback** | Optional behavior allowing a candidate from the next lower range when permitted by the effective rule |

Providers may have distinct image availability and reliability;
an enabled source does not guarantee a qualifying candidate.
Avoid inventing exact pixel cutoffs in documentation without
matching Config v5 and the application implementation.

## Selection and precedence

The current shared field contracts are documented in
[`schema/settings-ui.json`](../schema/settings-ui.json).
The native implementation includes
[`source_policy.rs`](../build/Windows/src/source_policy.rs)
and
[`strict_source_policy.rs`](../build/Windows/src/strict_source_policy.rs);
WebGUI's decoded-content check is implemented in
[`splined_strict_source_policy.py`](../build/WEBGui/webgui/backend/splined_strict_source_policy.py).

Source-specific settings should remain saved when a user temporarily
turns Source Override off. Strict Override enforces its decoded
front-cover/content rule and does not grant permission to silently
erase or rewrite custom provider settings. MusicBrainz identity
and Cover Art Archive exact-release evidence require particular
care; absence of verified front-art evidence is not permission
to label arbitrary imagery authoritative.

Explicit candidates below a user's ordinary minimum, where a
supported runtime override exists, must warn and remain limited
to the current attempt rather than silently changing the
persistent Config v5 contract. Fallback/COMP Album selection and
embedded-art size checks must follow their separately approved
identity and writer policies.

## Upscaling and source provenance

Conventional image enlargement and optional AISPLINED model-based
remediation are different operations. Preview percentages,
minimum/ideal floors, allowed interpolation and chosen output
dimensions must be reported using actual processed images.
An enhanced image may need separately preserved provenance;
"previously enhanced" must not be inferred from resolution
alone. Upscaling a preview is not a music-file WRITE.

The approved visual status of an Artist/Album NAME, its Album
status dots, override frame and COMP-track row does not change
just because an image candidate has a preferred provider range.
Those are separate state classifications in the
[policy register](POLICY-REGISTER.md).

## Validation and related guides

Exact per-source defaults, advanced dimensions, setting visibility
and saved behavior require independent runtime checks in Windows
and WebGUI. This guide restores verified vocabulary and source
boundaries, not an acceptance certificate for every historical
control. Use [Config v5](config-v5-reference.md),
[Windows guide](windows-guide.md),
[WebGUI guide](browser-workspace-guide.md) and
[Credentials](credentials-providers.md) for the adjacent workflows.
