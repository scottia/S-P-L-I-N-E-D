"""Single-admin authentication for the browser workspace."""

from __future__ import annotations

import base64
import hashlib
import hmac
import re
import secrets
from contextlib import closing

from webgui.database import Database

_USERNAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{2,63}$")
_ALGORITHM = "scrypt-n16384-r8-p1-v1"
_DUMMY_SALT = b"SPLINED browser authentication"


def _password_hash(password: str, salt: bytes) -> bytes:
    return hashlib.scrypt(
        password.encode("utf-8"),
        salt=salt,
        n=16384,
        r=8,
        p=1,
        dklen=32,
        maxmem=64 * 1024 * 1024,
    )


class AdminAuth:
    """Persist one password-hashed administrator in PostgreSQL.

    The deployment token is used only to authorize first-run enrollment and is
    never copied into PostgreSQL.
    """

    def __init__(self, database: Database):
        self.database = database

    def account_exists(self) -> bool:
        with closing(self.database.connect()) as db:
            return db.execute("SELECT 1 FROM web_admin_accounts LIMIT 1").fetchone() is not None

    @staticmethod
    def validate_username(username: str) -> str:
        value = username.strip()
        if not _USERNAME.fullmatch(value):
            raise ValueError(
                "Username must be 3–64 characters using letters, numbers, dot, underscore, or hyphen"
            )
        return value

    @staticmethod
    def validate_password(password: str) -> str:
        if not 10 <= len(password) <= 256:
            raise ValueError("Password must contain 10–256 characters")
        if not any(character.isalpha() for character in password) or not any(
            character.isdigit() for character in password
        ):
            raise ValueError("Password must include at least one letter and one number")
        return password

    def setup(
        self,
        username: str,
        password: str,
        confirmation: str,
        supplied_token: str,
        deployment_token: str,
    ) -> str:
        username = self.validate_username(username)
        password = self.validate_password(password)
        if not hmac.compare_digest(password, confirmation):
            raise ValueError("Password confirmation does not match")
        if not deployment_token or not hmac.compare_digest(supplied_token, deployment_token):
            raise PermissionError("The WebGUI setup token is incorrect")
        salt = secrets.token_bytes(24)
        digest = _password_hash(password, salt)
        with closing(self.database.connect()) as db, db:
            db.execute("SELECT pg_advisory_xact_lock(?)", (741520206,))
            if db.execute("SELECT 1 FROM web_admin_accounts LIMIT 1").fetchone():
                raise ValueError("The WebGUI administrator has already been created")
            db.execute(
                "INSERT INTO web_admin_accounts(username_key,username,password_algorithm,password_salt,password_hash) "
                "VALUES(?,?,?,?,?)",
                (
                    username.casefold(),
                    username,
                    _ALGORITHM,
                    base64.b64encode(salt).decode("ascii"),
                    base64.b64encode(digest).decode("ascii"),
                ),
            )
        return username

    def authenticate(self, username: str, password: str) -> str | None:
        key = username.strip().casefold()
        with closing(self.database.connect()) as db:
            row = db.execute(
                "SELECT username,password_algorithm,password_salt,password_hash "
                "FROM web_admin_accounts WHERE username_key=?",
                (key,),
            ).fetchone()
        try:
            salt = base64.b64decode(row["password_salt"], validate=True) if row else _DUMMY_SALT
            expected = base64.b64decode(row["password_hash"], validate=True) if row else bytes(32)
            actual = _password_hash(password[:256], salt)
        except (TypeError, ValueError):
            actual, expected = _password_hash(password[:256], _DUMMY_SALT), bytes(32)
        valid = bool(row and row["password_algorithm"] == _ALGORITHM and hmac.compare_digest(actual, expected))
        if not valid:
            return None
        with closing(self.database.connect()) as db, db:
            db.execute(
                "UPDATE web_admin_accounts SET last_login_at=now() WHERE username_key=?",
                (key,),
            )
        return str(row["username"])
