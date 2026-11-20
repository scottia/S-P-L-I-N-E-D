"""Browser credential editing never returns stored secrets to the client."""

from __future__ import annotations

import json
import os

import splined as core
import splined_oauth_validation as validation


FIELDS = {
    "navidrome": ("server_url", "auth_method", "username", "password", "api_key"),
    "fanarttv": ("api_key", "client_key"),
    "lastfm": ("api_key", "shared_secret", "username", "session_key"),
    "discogs": ("token",),
    "musicbrainz": (
        "client_id",
        "client_secret",
        "callback_uri",
        "oauth_scope",
        "access_token",
        "refresh_token",
    ),
}
PUBLIC = {
    "server_url",
    "auth_method",
    "username",
    "client_id",
    "callback_uri",
    "oauth_scope",
}


class Credentials:
    def __init__(self, runtime):
        self.runtime = runtime

    def read(self, provider):
        if provider not in FIELDS:
            raise ValueError("Unknown credential provider")
        path = core.credential_file(
            self.runtime.config_file, self.runtime.cfg, provider
        )
        return json.loads(path.read_text(encoding="utf-8")) if path.is_file() else {}

    def describe(self, provider):
        stored = self.read(provider)
        return {
            "provider": provider,
            "fields": [
                {
                    "name": name,
                    "secret": name not in PUBLIC,
                    "value": stored.get(name, "") if name in PUBLIC else "",
                    "configured": bool(stored.get(name)),
                }
                for name in FIELDS[provider]
            ],
        }

    def save(self, provider, values):
        stored = self.read(provider)
        if not isinstance(values, dict):
            raise ValueError("Credential fields must be an object")
        for key, value in values.items():
            if (
                key not in FIELDS[provider]
                or not isinstance(value, str)
                or len(value) > 8192
            ):
                raise ValueError("Invalid credential field")
            if value or key in PUBLIC:
                stored[key] = value
        if provider == "navidrome":
            from urllib.parse import urlsplit

            parsed = urlsplit(str(stored.get("server_url", "")))
            if parsed.scheme not in {"http", "https"} or not parsed.netloc:
                raise ValueError("Navidrome requires an HTTP or HTTPS server address")
            mode = stored.get(
                "auth_method", "api_key" if stored.get("api_key") else "password"
            )
            if mode == "api_key" and not stored.get("api_key"):
                raise ValueError("Enter a Navidrome API key")
            if mode == "password" and not (
                stored.get("username") and stored.get("password")
            ):
                raise ValueError("Enter a Navidrome username and password")
        if provider == "fanarttv":
            stored["api_version"] = "v3.2"
        path = core.credential_file(
            self.runtime.config_file, self.runtime.cfg, provider
        )
        path.parent.mkdir(parents=True, exist_ok=True)
        core.save_json_atomic(path, stored)
        if os.name != "nt":
            path.chmod(0o600)
        return self.describe(provider)

    def test(self, provider):
        if provider == "navidrome":
            credential = core.load_navidrome_credentials(
                self.runtime.config_file, self.runtime.cfg
            )
            core._navidrome_json(
                core._navidrome_get(core.Http(), credential, "ping", {})
            )
            return {"ok": True, "message": "Navidrome connected"}
        methods = {
            "discogs": validation._validate_discogs,
            "fanarttv": validation._validate_fanarttv,
            "lastfm": validation._validate_lastfm,
            "musicbrainz": validation._validate_musicbrainz,
        }
        if provider not in methods:
            raise ValueError("Unknown credential provider")
        state = methods[provider](self.runtime.config_file, self.runtime.cfg)
        return {"ok": state == "PASS", "message": str(state)}

    def authorize_musicbrainz(self, session):
        import secrets, hashlib, time
        from urllib.parse import urlencode

        stored = self.read("musicbrainz")
        client_id = str(stored.get("client_id", "")).strip()
        if not client_id:
            raise ValueError("Save a MusicBrainz client ID first")
        verifier = secrets.token_urlsafe(48)
        state = secrets.token_urlsafe(32)
        redirect = str(stored.get("callback_uri") or "urn:ietf:wg:oauth:2.0:oob")
        session["musicbrainz_oauth"] = {
            "verifier": verifier,
            "state": state,
            "created": time.time(),
            "redirect": redirect,
            "client_id": client_id,
        }
        import base64

        challenge = (
            base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest())
            .decode()
            .rstrip("=")
        )
        return {
            "url": core.MB_AUTHORIZE_URL
            + "?"
            + urlencode(
                {
                    "response_type": "code",
                    "client_id": client_id,
                    "redirect_uri": redirect,
                    "scope": stored.get("oauth_scope") or "profile",
                    "state": state,
                    "code_challenge": challenge,
                    "code_challenge_method": "S256",
                }
            ),
            "state": state,
        }

    def complete_musicbrainz(self, session, value):
        import time, hmac, requests

        flow = session.get("musicbrainz_oauth")
        if (
            not flow
            or time.time() - flow["created"] > 600
            or not hmac.compare_digest(str(value.get("state", "")), flow["state"])
        ):
            raise ValueError(
                "Authorization expired; start MusicBrainz authorization again"
            )
        code = str(value.get("code", "")).strip()
        if not code or len(code) > 8192:
            raise ValueError("Paste the MusicBrainz authorization code")
        stored = self.read("musicbrainz")
        try:
            response = requests.post(
                core.MB_OAUTH_ENDPOINT,
                data={
                    "grant_type": "authorization_code",
                    "code": code,
                    "client_id": flow["client_id"],
                    "client_secret": stored.get("client_secret", ""),
                    "redirect_uri": flow["redirect"],
                    "code_verifier": flow["verifier"],
                },
                headers={"User-Agent": core.USER_AGENT},
                timeout=core.REQUEST_TIMEOUT,
            )
            if not response.ok:
                raise ValueError(
                    "MusicBrainz rejected the authorization code; authorize again"
                )
            token = response.json()
        except requests.RequestException:
            raise ValueError(
                "MusicBrainz authorization could not connect; try again"
            ) from None
        if not token.get("access_token") or int(token.get("expires_in", 0)) <= 0:
            raise ValueError("MusicBrainz returned an unusable token")
        stored.update(
            access_token=token["access_token"],
            refresh_token=token.get("refresh_token", ""),
            token_type=token.get("token_type", "Bearer"),
            scope=token.get("scope", "profile"),
            expires_at_unix=int(time.time()) + int(token["expires_in"]),
            oauth_enabled=True,
        )
        core.save_json_atomic(
            core.credential_file(
                self.runtime.config_file, self.runtime.cfg, "musicbrainz"
            ),
            stored,
        )
        session.pop("musicbrainz_oauth", None)
        return {"ok": True, "message": "MusicBrainz authorization saved"}
