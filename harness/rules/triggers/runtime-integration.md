# Runtime And Integration Trigger

## Required Coverage
- Design records lifecycle, concurrency, ordering, timeout, retry, cancellation, dependency availability, observability, and recovery behavior affected by the change.
- Testing includes the applicable failure workflow and a DV or integration path, or a concrete manual/disabled reason with owner and acceptance impact.

## Reviewer Focus
- Check races, stuck work, duplicate side effects, resource leaks, missing backpressure, retry storms, and unclear operational recovery.
