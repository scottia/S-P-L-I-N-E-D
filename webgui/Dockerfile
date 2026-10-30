FROM python:3.12-slim-bookworm
WORKDIR /app
COPY webgui/requirements.txt /app/requirements.txt
RUN pip install --no-cache-dir -r /app/requirements.txt \
 && groupadd --gid 1000 splined \
 && useradd --uid 1000 --gid 1000 --no-create-home splined \
 && mkdir -p /config /_cache /_logs /credentials \
 && chown splined:splined /config /_cache /_logs /credentials
COPY webgui /app/webgui
COPY schema/media.sql schema/settings-ui.json /app/schema/
COPY docker/config.example.toml /app/docker/config.example.toml
ENV PYTHONUNBUFFERED=1 PYTHONDONTWRITEBYTECODE=1 SPLINED_WEB_HOST=0.0.0.0 SPLINED_WEB_PORT=8080 SPLINED_CONFIG=/config/config.toml
USER splined
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s CMD python -c "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8080/api/status',timeout=4)"
CMD ["python", "-m", "webgui"]
