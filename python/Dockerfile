FROM rust:1.90-slim-bookworm AS pyratatui-input-builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 python3-venv \
    && rm -rf /var/lib/apt/lists/* \
    && python3 -m venv /opt/maturin

RUN /opt/maturin/bin/pip install --disable-pip-version-check --no-cache-dir maturin==1.9.6

COPY pyratatui_input /build/pyratatui_input
RUN /opt/maturin/bin/maturin build \
        --manifest-path /build/pyratatui_input/Cargo.toml \
        --release \
        --out /wheels


FROM python:3.13-slim-bookworm@sha256:2325bb286ec344af3e5898cc224b5844e2707ac6e26b1632516fd3edc84a5e26

ARG SPLINED_BUILD_CHANNEL=dev

RUN apt-get update \
    && apt-get install -y --no-install-recommends micro \
    && rm -rf /var/lib/apt/lists/*

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PYTHONPATH=/app \
    SPLINED_BUILD_CHANNEL=${SPLINED_BUILD_CHANNEL} \
    SPLINED_CONFIG=/config/config.toml \
    SPLINED_LOG_DIR=/_logs \
    SPLINED_SQLITE_LOCAL_SNAPSHOT=always \
    SPLINED_COLOR=always

WORKDIR /app

COPY requirements.txt /app/requirements.txt
RUN pip install --disable-pip-version-check --no-cache-dir -r /app/requirements.txt
COPY --from=pyratatui-input-builder /wheels /wheels
RUN pip install --disable-pip-version-check --no-cache-dir /wheels/*.whl \
    && rm -rf /wheels

COPY sitecustomize.py /app/sitecustomize.py
COPY splined_schema.sql /app/splined_schema.sql
COPY splined_media_tags.py /app/splined_media_tags.py
COPY splined_media_database.py /app/splined_media_database.py
COPY splined_media_runtime.py /app/splined_media_runtime.py
COPY splined_media_index.py /app/splined_media_index.py
COPY splined_media_build_policy.py /app/splined_media_build_policy.py
COPY splined_media_fast_index_policy.py /app/splined_media_fast_index_policy.py
COPY splined_media_finalize_policy.py /app/splined_media_finalize_policy.py
COPY splined_media_load_policy.py /app/splined_media_load_policy.py
COPY splined_media_warm_start_policy.py /app/splined_media_warm_start_policy.py
COPY splined_media_checkpoint_policy.py /app/splined_media_checkpoint_policy.py
COPY splined_compilation_authority_policy.py /app/splined_compilation_authority_policy.py
COPY splined_tui_progress_policy.py /app/splined_tui_progress_policy.py
COPY splined_ranking_policy.py /app/splined_ranking_policy.py
COPY splined_strict_source_policy.py /app/splined_strict_source_policy.py
COPY splined_tui_overlay_policy.py /app/splined_tui_overlay_policy.py
COPY splined_entry.py /app/splined_entry.py
COPY splined.py /app/splined.py
COPY splined_scan.py /app/splined_scan.py
COPY splined_oauth_validation.py /app/splined_oauth_validation.py
COPY tui /app/tui
COPY test_candidate_ranking_policy.py /tmp/test_candidate_ranking_policy.py
COPY test_strict_source_policy.py /tmp/test_strict_source_policy.py
COPY test_splined_media_index.py /tmp/test_splined_media_index.py
COPY test_splined_media_build_policy.py /tmp/test_splined_media_build_policy.py
COPY test_splined_media_fast_index_policy.py /tmp/test_splined_media_fast_index_policy.py
COPY test_splined_media_finalize_policy.py /tmp/test_splined_media_finalize_policy.py
COPY test_splined_media_load_policy.py /tmp/test_splined_media_load_policy.py
COPY test_splined_media_warm_start_policy.py /tmp/test_splined_media_warm_start_policy.py
COPY test_splined_media_checkpoint_policy.py /tmp/test_splined_media_checkpoint_policy.py
COPY test_splined_compilation_authority_policy.py /tmp/test_splined_compilation_authority_policy.py
COPY test_splined_scan_lazy_batch.py /tmp/test_splined_scan_lazy_batch.py
RUN python3 -m py_compile \
        /app/sitecustomize.py \
        /app/splined_media_tags.py \
        /app/splined_media_database.py \
        /app/splined_media_runtime.py \
        /app/splined_media_index.py \
        /app/splined_media_build_policy.py \
        /app/splined_media_fast_index_policy.py \
        /app/splined_media_finalize_policy.py \
        /app/splined_media_load_policy.py \
        /app/splined_media_warm_start_policy.py \
        /app/splined_media_checkpoint_policy.py \
        /app/splined_compilation_authority_policy.py \
        /app/splined_tui_progress_policy.py \
        /app/splined_ranking_policy.py \
        /app/splined_strict_source_policy.py \
        /app/splined_tui_overlay_policy.py \
        /app/splined_entry.py \
        /app/splined.py \
        /app/splined_scan.py \
        /app/splined_oauth_validation.py \
    && python3 -m compileall -q /app/tui \
    && cd /tmp \
    && python3 -m unittest -v \
        test_candidate_ranking_policy \
        test_strict_source_policy \
        test_splined_media_index \
        test_splined_media_build_policy \
        test_splined_media_fast_index_policy \
        test_splined_media_finalize_policy \
        test_splined_media_load_policy \
        test_splined_media_warm_start_policy \
        test_splined_media_checkpoint_policy \
        test_splined_compilation_authority_policy \
        test_splined_scan_lazy_batch \
    && rm -f \
        /tmp/test_candidate_ranking_policy.py \
        /tmp/test_strict_source_policy.py \
        /tmp/test_splined_media_index.py \
        /tmp/test_splined_media_build_policy.py \
        /tmp/test_splined_media_fast_index_policy.py \
        /tmp/test_splined_media_finalize_policy.py \
        /tmp/test_splined_media_load_policy.py \
        /tmp/test_splined_media_warm_start_policy.py \
        /tmp/test_splined_media_checkpoint_policy.py \
        /tmp/test_splined_compilation_authority_policy.py \
        /tmp/test_splined_scan_lazy_batch.py \
    && chmod 0444 /app/splined_schema.sql \
    && chmod 0555 \
        /app/sitecustomize.py \
        /app/splined_media_tags.py \
        /app/splined_media_database.py \
        /app/splined_media_runtime.py \
        /app/splined_media_index.py \
        /app/splined_media_build_policy.py \
        /app/splined_media_fast_index_policy.py \
        /app/splined_media_finalize_policy.py \
        /app/splined_media_load_policy.py \
        /app/splined_media_warm_start_policy.py \
        /app/splined_media_checkpoint_policy.py \
        /app/splined_compilation_authority_policy.py \
        /app/splined_tui_progress_policy.py \
        /app/splined_ranking_policy.py \
        /app/splined_strict_source_policy.py \
        /app/splined_tui_overlay_policy.py \
        /app/splined_entry.py \
        /app/splined.py \
        /app/splined_scan.py \
        /app/splined_oauth_validation.py \
    && find /app/tui -type f -name '*.py' -exec chmod 0555 {} + \
    && ln -s /app/splined_entry.py /usr/local/bin/splined

# Persistent media index and all durable runtime state live in /_cache/splined.db.
VOLUME ["/config", "/music", "/_cache", "/_logs", "/credentials"]

ENTRYPOINT ["python3", "/app/splined_entry.py"]
CMD ["--idle"]
