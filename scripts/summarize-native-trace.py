#!/usr/bin/env python3
"""Summarize opt-in numeric native CPU/GPU traces; never prints input values."""
import argparse
import json
import math
from pathlib import Path
import statistics

COUNTS = ('keys', 'text_events', 'pastes', 'scrolls', 'pointer_events')
FIELDS = set(COUNTS) | {'sequence', 'cpu_to_paint_us', 'gpu_draw_ns',
                        'gpu_timer_supported', 'dropped_samples'}
MAX_TRACE_BYTES = 16 * 1024 * 1024
MAX_RECORD_BYTES = 2048


def read_samples(path):
    samples = []
    total = 0
    with path.open(encoding='utf-8') as stream:
        number = 0
        while line := stream.readline(MAX_RECORD_BYTES + 1):
            number += 1
            total += len(line.encode('utf-8'))
            if len(line) > MAX_RECORD_BYTES or total > MAX_TRACE_BYTES:
                raise ValueError('numeric trace exceeds its size limit')
            # A live writer or SIGKILL may leave one incomplete final record.
            if not line.endswith('\n'):
                break
            try:
                row = json.loads(line)
                if not isinstance(row, dict) or set(row) != FIELDS:
                    raise ValueError()
                if type(row['gpu_timer_supported']) is not bool:
                    raise ValueError()
                for key in FIELDS - {'gpu_timer_supported'}:
                    value = row[key]
                    if key == 'gpu_draw_ns' and value is None:
                        continue
                    if type(value) is not int or not 0 <= value <= 2**64 - 1:
                        raise ValueError()
            except (ValueError, TypeError) as error:
                raise ValueError(f'invalid numeric trace record at line {number}') from error
            samples.append(row)
    return samples


def timing(values, divisor):
    if not values:
        return None
    values = sorted(values)
    return dict(n=len(values), p50_ms=round(statistics.median(values)/divisor, 4),
                p95_ms=round(values[math.ceil(len(values)*.95)-1]/divisor, 4))


def summarize(samples):
    groups = {}
    for row in samples:
        group = next((name for key, name in (
            ('scrolls', 'scroll'), ('pastes', 'paste'), ('text_events', 'typing'),
            ('keys', 'commands'), ('pointer_events', 'pointer')) if row[key]), 'no_input')
        groups.setdefault(group, []).append(row)
    return dict(samples=len(samples),
                dropped_samples=max((r['dropped_samples'] for r in samples), default=0),
                gpu_api_supported=any(r['gpu_timer_supported'] for r in samples),
                groups={name: dict(cpu_input_batch_to_paint=timing([r['cpu_to_paint_us'] for r in rows], 1000),
                                   gpu_draw=timing([r['gpu_draw_ns'] for r in rows if r['gpu_draw_ns'] is not None], 1e6))
                        for name, rows in sorted(groups.items())})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('trace', type=Path)
    parser.add_argument('--after-sequence', type=int, default=-1)
    parser.add_argument('--through-sequence', type=int, default=2**64-1)
    args = parser.parse_args()
    rows = [row for row in read_samples(args.trace)
            if args.after_sequence < row['sequence'] <= args.through_sequence]
    summary = summarize(rows)
    summary['trace_bytes'] = args.trace.stat().st_size
    summary['near_size_limit'] = summary['trace_bytes'] > MAX_TRACE_BYTES - MAX_RECORD_BYTES
    print(json.dumps(summary, indent=2, allow_nan=False))


if __name__ == '__main__':
    main()
