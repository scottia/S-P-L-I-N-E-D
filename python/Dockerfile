FROM python:3.14-slim-bookworm@sha256:82bc3c539b8813ada9d68c63b40158fa002f7f33de9bf3312a3dfdc0620dff56

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
RUN python3 -m py_compile \
        /app/splined.py \
        /app/splined_scan.py \
        /app/splined_oauth_validation.py \
    && chmod 0555 \
        /app/splined.py \
        /app/splined_scan.py \
        /app/splined_oauth_validation.py \
    && ln -s /app/splined_scan.py /usr/local/bin/splined

# Persistent runtime data; history lives under /_logs/_history.
VOLUME ["/config", "/music", "/_cache", "/_logs", "/credentials"]

ENTRYPOINT ["python3", "/app/splined_scan.py"]
CMD ["--idle"]
