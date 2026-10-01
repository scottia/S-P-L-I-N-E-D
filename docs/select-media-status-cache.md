# Select Media status authority

Select Media state is stored in `<scan.cache_dir>/splined.db`.

The `albums` and `artists` tables provide the current picker projection. Album
completion, bypass, timeout, incomplete compilation progress, and selected
source are updated transactionally with the matching Album row. `cache_history`
provides the audit trail, while durable auxiliary state such as source-ranking
counts uses `cache_entries`.

SPLINED does not create, read, migrate, or update JSON status/history files.
Removing diagnostic logs does not change Album status. Removing `splined.db`
removes the index and all durable runtime state and therefore requires a fresh
Python library index before a shared Windows client can load it.
