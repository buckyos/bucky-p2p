# Data, Schema, And Migration Trigger

## Required Coverage
- Proposal or design states migration, rollback, reset, retention, and mixed-version expectations for persisted data, schemas, serialized state, cache keys, indexes, and defaults.
- Testing covers old data, new data, partial migration failure, rollback, and mixed-version or reset paths where relevant.

## Reviewer Focus
- Check irreversible writes, stale readers, downgrade behavior, backup/recovery impact, and data-loss risk.
