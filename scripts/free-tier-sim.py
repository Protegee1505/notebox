#!/usr/bin/env python3
"""Simulate one heavy day against the Cloudflare free-tier limits.

This is a model built from the endpoint design, not a measurement. S3 on the
deployed Worker is what proves the 10 ms CPU budget; this script proves the
request, row and size budgets before anything is deployed.

Limits checked against Cloudflare docs on 2026-10-20:
  Workers: 100,000 requests/day, 10 ms CPU, 50 subrequests/request
  D1: 5,000,000 rows read/day, 100,000 rows written/day, 50 queries/invocation,
      100 bound parameters, 2 MB rows, 500 MB per free database

Run from any directory: python3 scripts/free-tier-sim.py
Exit code is non-zero when a daily limit exceeds 10% or a structural limit
does not fit.
"""

from __future__ import annotations

import sys
from dataclasses import dataclass

# The plan's gate: one heavy day must stay under 10% of every daily limit.
DAILY_CEILING = 0.10

LIMITS = {
    "worker_requests": 100_000,
    "rows_read": 5_000_000,
    "rows_written": 100_000,
    "queries_per_request": 50,
    "bound_parameters": 100,
    "row_bytes": 2_000_000,
    "request_body_bytes": 100 * 1024 * 1024,
    "database_bytes": 500 * 1024 * 1024,
}

# Conservative wire sizes. Base64 inflates by 4/3; the server stores updates as
# base64 text and attachment chunks as BLOBs.
MAX_UPDATE_BYTES = 1024 * 1024
MAX_CHUNK_BYTES = 1024 * 1024
BASE64_OVERHEAD = 4 / 3


@dataclass(frozen=True)
class Workload:
    """One heavy day for one user on three devices."""

    devices: int = 3
    visible_hours_per_device: int = 8
    saves_per_day: int = 500
    logins_per_day: int = 3
    device_lists_per_day: int = 10
    photos_uploaded: int = 10
    photos_downloaded: int = 15
    average_photo_bytes: int = 400 * 1024
    average_update_bytes: int = 250
    poll_seconds: int = 10
    touch_seconds: int = 300


def simulate(work: Workload) -> dict[str, float]:
    """Return modelled daily totals and per-request maxima."""
    polls = work.devices * work.visible_hours_per_device * 3600 // work.poll_seconds
    focus_syncs = work.devices * 60
    pulls = polls + focus_syncs
    sync_requests = pulls + work.saves_per_day
    blob_requests = work.photos_uploaded + work.photos_downloaded
    requests = (
        sync_requests + blob_requests + work.logins_per_day + work.device_lists_per_day
    )

    # Each sync: one device lookup, then one pull query. A pull returns up to
    # two rows in steady state; a push adds no reads.
    rows_read_pulls = pulls * (1 + 2)
    rows_read_pushes = work.saves_per_day
    rows_read_logins = work.logins_per_day
    rows_read_device_lists = work.device_lists_per_day * (1 + work.devices)
    rows_read_blobs = work.photos_downloaded * 2 + work.photos_uploaded
    rows_read = (
        rows_read_pulls
        + rows_read_pushes
        + rows_read_logins
        + rows_read_device_lists
        + rows_read_blobs
    )

    # One update row plus its AUTOINCREMENT sequence entry.
    update_writes = work.saves_per_day * 2
    touch_writes = (
        work.devices
        * work.visible_hours_per_device
        * 3600
        // work.touch_seconds
    )
    login_writes = work.logins_per_day * (1 + 1)
    blob_writes = work.photos_uploaded * (1 + 1)
    rows_written = update_writes + touch_writes + login_writes + blob_writes

    # Worst-case single request: auth, 25 batched inserts, one pull page and a
    # throttled last_seen update.
    queries_per_push = 1 + 25 + 1 + 1
    queries_per_blob = 1 + 1 + 1 + 1
    bound_parameters = 5
    update_row_bytes = int(MAX_UPDATE_BYTES * BASE64_OVERHEAD) + 1_024
    chunk_row_bytes = MAX_CHUNK_BYTES + 1_024

    # Growth on the heavy day and after a year of such days, for information.
    notes_daily_bytes = work.saves_per_day * (work.average_update_bytes + 100)
    blob_daily_bytes = work.photos_uploaded * work.average_photo_bytes
    photos_fit = LIMITS["database_bytes"] // work.average_photo_bytes

    return {
        "requests": requests,
        "rows_read": rows_read,
        "rows_written": rows_written,
        "queries_per_push": queries_per_push,
        "queries_per_blob": queries_per_blob,
        "bound_parameters": bound_parameters,
        "update_row_bytes": update_row_bytes,
        "chunk_row_bytes": chunk_row_bytes,
        "notes_daily_bytes": notes_daily_bytes,
        "blob_daily_bytes": blob_daily_bytes,
        "notes_yearly_bytes": notes_daily_bytes * 365,
        "photos_fit": photos_fit,
        "sync_requests": sync_requests,
        "blob_requests": blob_requests,
        "pulls": pulls,
    }


def check_daily(label: str, value: float, limit: int) -> bool:
    share = value / limit
    status = "PASS" if share <= DAILY_CEILING else "FAIL"
    print(
        f"  {label:<22} {value:>10,.0f} / {limit:>10,}  "
        f"{share:>7.2%}  {status}"
    )
    return share <= DAILY_CEILING


def check_limit(label: str, value: float, limit: float) -> bool:
    status = "PASS" if value <= limit else "FAIL"
    print(f"  {label:<22} {value:>10,.0f} / {limit:>10,.0f}  {status}")
    return value <= limit


def main() -> int:
    work = Workload()
    result = simulate(work)
    failures = 0

    print("notebox free-tier simulator")
    print(
        "heavy day: "
        f"{work.devices} devices × {work.visible_hours_per_device} h, "
        f"{work.saves_per_day} saves, {work.photos_uploaded} photos up, "
        f"{work.photos_downloaded} down"
    )
    print(
        f"  requests: {result['requests']:,.0f} "
        f"({result['sync_requests']:,.0f} sync, {result['blob_requests']:,.0f} blob)"
    )

    print("\nDaily limits (gate: under 10%):")
    failures += not check_daily(
        "Workers requests", result["requests"], LIMITS["worker_requests"]
    )
    failures += not check_daily("D1 rows read", result["rows_read"], LIMITS["rows_read"])
    failures += not check_daily(
        "D1 rows written", result["rows_written"], LIMITS["rows_written"]
    )

    print("\nPer-request and row limits:")
    failures += not check_limit(
        "queries, 25-update push",
        result["queries_per_push"],
        LIMITS["queries_per_request"],
    )
    failures += not check_limit(
        "queries, blob chunk",
        result["queries_per_blob"],
        LIMITS["queries_per_request"],
    )
    failures += not check_limit(
        "bound parameters", result["bound_parameters"], LIMITS["bound_parameters"]
    )
    failures += not check_limit(
        "update row (base64)",
        result["update_row_bytes"],
        LIMITS["row_bytes"],
    )
    failures += not check_limit(
        "chunk row (BLOB)", result["chunk_row_bytes"], LIMITS["row_bytes"]
    )

    print("\nDatabase growth (informational):")
    notes_share = result["notes_yearly_bytes"] / LIMITS["database_bytes"]
    print(
        f"  notes DB after 1 year   {result['notes_yearly_bytes'] / 1024 / 1024:>8.1f} MB "
        f"/ {LIMITS['database_bytes'] / 1024 / 1024:.0f} MB  ({notes_share:.1%})"
    )
    print(
        f"  blobs on a heavy day    {result['blob_daily_bytes'] / 1024 / 1024:>8.1f} MB "
        f"/ {LIMITS['database_bytes'] / 1024 / 1024:.0f} MB"
    )
    print(
        f"  photos at {work.average_photo_bytes // 1024} KB     "
        f"{result['photos_fit']:>8,.0f} fit in the 500 MB blobs database"
    )
    if result["notes_yearly_bytes"] > LIMITS["database_bytes"]:
        print("  FAIL: notes alone fill the database within a year")
        failures += 1
    if result["blob_daily_bytes"] * 365 > LIMITS["database_bytes"]:
        print(
            "  note: a photo every day at this size passes 500 MB in "
            f"{LIMITS['database_bytes'] / result['blob_daily_bytes']:.0f} days; "
            "P6 adds the usage meter"
        )

    if failures:
        print(f"\nFAIL: {failures} limit(s) exceeded")
        return 1
    print("\nPASS: every daily limit stays under 10% and every row fits")
    return 0


if __name__ == "__main__":
    sys.exit(main())
