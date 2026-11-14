#!/usr/bin/env python3
"""Check numeric trace validation and summaries without a graphical display."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('native_trace', Path(__file__).with_name('summarize-native-trace.py'))
trace = importlib.util.module_from_spec(spec)
spec.loader.exec_module(trace)


class NumericTraceTests(unittest.TestCase):
    def sample(self):
        return dict(sequence=1, keys=1, text_events=1, pastes=0, scrolls=0,
                    pointer_events=0, cpu_to_paint_us=2500, gpu_draw_ns=4500000123,
                    gpu_timer_supported=True, dropped_samples=0)

    def read(self, data):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'trace.jsonl'
            path.write_text(data, encoding='utf-8')
            return trace.read_samples(path)

    def test_64_bit_gpu_times_and_quantiles(self):
        rows = [dict(self.sample(), cpu_to_paint_us=n * 1000) for n in (1, 2, 3, 4)]
        result = trace.summarize(self.read(''.join(json.dumps(r) + '\n' for r in rows)))
        timing = result['groups']['typing']
        self.assertEqual(timing['cpu_input_batch_to_paint'], dict(n=4, p50_ms=2.5, p95_ms=4.0))
        self.assertEqual(timing['gpu_draw']['p50_ms'], 4500.0001)

    def test_only_final_incomplete_record_is_ignored(self):
        line = json.dumps(self.sample()) + '\n'
        self.assertEqual(len(self.read(line + '{"sequence":')), 1)
        with self.assertRaisesRegex(ValueError, 'line 2'):
            self.read(line + '{\n' + line)

    def test_rejects_extra_fields_wrong_types_and_out_of_range_values(self):
        for change in ({'source': 'PRIVATE_DRAFT'}, {'keys': True}, {'gpu_draw_ns': -1},
                       {'cpu_to_paint_us': 2**64}, {'sequence': 1.5}, {'gpu_timer_supported': 1}):
            with self.subTest(change=change), self.assertRaises(ValueError) as error:
                self.read(json.dumps(dict(self.sample(), **change)) + '\n')
            self.assertNotIn('PRIVATE_DRAFT', str(error.exception))

    def test_record_limit_bounds_even_incomplete_input(self):
        with self.assertRaisesRegex(ValueError, 'size limit'):
            self.read('x' * (trace.MAX_RECORD_BYTES + 1))


if __name__ == '__main__':
    unittest.main()
