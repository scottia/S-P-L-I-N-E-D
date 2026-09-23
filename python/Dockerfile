FROM python:3.13-slim-bookworm@sha256:2325bb286ec344af3e5898cc224b5844e2707ac6e26b1632516fd3edc84a5e26

RUN apt-get update \
    && apt-get install -y --no-install-recommends micro \
    && rm -rf /var/lib/apt/lists/*

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    SPLINED_CONFIG=/config/config.toml \
    SPLINED_LOG_DIR=/_logs \
    SPLINED_HISTORY_DIR=/_logs/_history \
    SPLINED_COLOR=always

WORKDIR /app

COPY requirements.txt /app/requirements.txt
RUN pip install --disable-pip-version-check --no-cache-dir -r /app/requirements.txt

COPY splined.py /app/splined.py
COPY splined_scan.py /app/splined_scan.py
COPY splined_oauth_validation.py /app/splined_oauth_validation.py
COPY tui /app/tui
RUN python3 -m py_compile \
        /app/splined.py \
        /app/splined_scan.py \
        /app/splined_oauth_validation.py \
    && python3 -m compileall -q /app/tui \
    && chmod 0555 \
        /app/splined.py \
        /app/splined_scan.py \
        /app/splined_oauth_validation.py \
    && find /app/tui -type f -name '*.py' -exec chmod 0555 {} + \
    && ln -s /app/splined_scan.py /usr/local/bin/splined

# Persistent runtime data; history lives under /_logs/_history.
VOLUME ["/config", "/music", "/_cache", "/_logs", "/credentials"]

ENTRYPOINT ["python3", "/app/splined_scan.py"]
CMD ["--idle"]
