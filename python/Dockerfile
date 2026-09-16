FROM python:3.13-slim-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends micro \
    && rm -rf /var/lib/apt/lists/*

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    SPLINED_CONFIG=/config/config.toml \
    SPLINED_COLOR=always

WORKDIR /app

COPY requirements.txt /app/requirements.txt
RUN pip install --no-cache-dir -r /app/requirements.txt

COPY splined.py /app/splined.py
COPY splined_scan.py /app/splined_scan.py
RUN python3 -m py_compile /app/splined.py /app/splined_scan.py \
    && chmod 0555 /app/splined.py /app/splined_scan.py \
    && ln -s /app/splined_scan.py /usr/local/bin/splined

VOLUME ["/config", "/music", "/cache", "/credentials"]

ENTRYPOINT ["python3", "/app/splined_scan.py"]
CMD ["--idle"]
