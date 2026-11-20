# PR intent: Preferred embedded-thumbnail size parity

Bootstrap only. Correct the selected Album embedded-artwork summary so above-preference and non-square thumbnails receive the same noncompliant attention as missing/below-preference thumbnails. Preserve current strict physical readback and retry semantics, independent status-dot/name priorities, frozen schema JSON, existing database behavior and all UI frames. Add focused tests for oversized, non-square and valid preference sizes; no production release, live data or /dev changes. Remove this temporary file after Draft PR creation.
