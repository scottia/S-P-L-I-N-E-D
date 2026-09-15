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
RUN chmod 0555 /app/splined.py \
    && ln -s /app/splined.py /usr/local/bin/splined

VOLUME ["/config", "/music", "/cache", "/credentials"]

ENTRYPOINT ["python3", "/app/splined.py"]
CMD ["--idle"]
