"""Deterministic stdio LSP fixture. Never used by production builds."""
import argparse
import json
import os
from pathlib import Path
import sys
import time

parser = argparse.ArgumentParser()
parser.add_argument('--record')
parser.add_argument('--label')
parser.add_argument('--encoding', default='utf-16')
parser.add_argument('--fragment', action='store_true')
parser.add_argument('--crash-on-open', action='store_true')
parser.add_argument('--hang-initialize', action='store_true')
parser.add_argument('--unsupported-definitions', action='store_true')
args = parser.parse_args()
record = open(args.record, 'a', encoding='utf-8') if args.record else None
if record:
    record.write(json.dumps({'method': 'fixture/environment', 'params': {'value': os.environ.get('HEDIT_LSP_TEST_FLAG')}}) + '\n')
    record.flush()

def send(value):
    data = json.dumps(dict(jsonrpc='2.0', **value), ensure_ascii=False).encode('utf-8')
    wire = f'Content-Length: {len(data)}\r\n\r\n'.encode() + data
    if args.fragment:
        for b in wire:
            sys.stdout.buffer.write(bytes([b]))
            sys.stdout.buffer.flush()
    else:
        sys.stdout.buffer.write(wire)
        sys.stdout.buffer.flush()

def read():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line == b'\r\n':
            break
        key, value = line.decode().split(':', 1)
        headers[key.lower()] = value.strip()
    return json.loads(sys.stdin.buffer.read(int(headers['content-length'])))

root = ''
while True:
    value = read()
    if value is None:
        break
    if record:
        record.write(json.dumps(value, ensure_ascii=False) + '\n')
        record.flush()
    method = value.get('method')
    params = value.get('params') or {}
    request_id = value.get('id')
    if method == 'initialize':
        root = params['rootUri']
        if args.hang_initialize:
            continue
        send({'id': 'configuration', 'method': 'workspace/configuration', 'params': {'items': [{'section': 'fixture'}]}})
        send({'id': request_id, 'result': {'capabilities': {
            'positionEncoding': args.encoding,
            'textDocumentSync': {'openClose': True, 'change': 2, 'save': {'includeText': True}},
            'completionProvider': {}, 'hoverProvider': True, 'definitionProvider': not args.unsupported_definitions,
            'referencesProvider': True, 'documentFormattingProvider': True,
        }}})
    elif method in ('textDocument/didOpen', 'textDocument/didChange'):
        if args.crash_on_open:
            sys.stderr.write('fixture intentional crash\n')
            sys.stderr.flush()
            sys.exit(7)
        doc = params['textDocument']
        # First an obsolete publication, then a current one.
        for version, message in [(doc['version'] - 1, 'obsolete diagnostic'), (doc['version'], 'fixture diagnostic')]:
            send({'method': 'textDocument/publishDiagnostics', 'params': {
                'uri': doc['uri'], 'version': version, 'diagnostics': [{
                    'range': {'start': {'line': 0, 'character': 0}, 'end': {'line': 0, 'character': 0}},
                    'severity': 2, 'message': message,
                }],
            }})
    elif method == 'textDocument/completion':
        pos = params['position']
        send({'id': request_id, 'result': {'items': [{
            'label': 'format_value', 'detail': 'Fixture completion',
            'textEdit': {'range': {'start': dict(pos, character=max(0, pos['character'] - 2)), 'end': pos}, 'newText': 'format_value'},
        }], 'isIncomplete': False}})
    elif method == 'textDocument/hover':
        send({'id': request_id, 'result': {'contents': {'kind': 'plaintext', 'value': args.label or 'Fixture hover information'}}})
    elif method == 'test/diagnostics':
        send({'method': 'textDocument/publishDiagnostics', 'params': params})
        send({'id': request_id, 'result': True})
    elif method == 'textDocument/definition':
        char = {'utf-8': 5, 'utf-16': 3, 'utf-32': 2}[args.encoding]
        start = {'line': 0, 'character': char}
        send({'id': request_id, 'result': [{'targetUri': root + '/target.rs', 'targetSelectionRange': {'start': start, 'end': start}, 'targetRange': {'start': start, 'end': start}}]})
    elif method == 'textDocument/references':
        send({'id': request_id, 'result': []})
    elif method == 'textDocument/formatting':
        pos = {'line': 0, 'character': 0}
        send({'id': request_id, 'result': [{'range': {'start': pos, 'end': pos}, 'newText': '// formatted\n'}]})
    elif method == 'test/hang':
        pass
    elif method == 'test/log-flood':
        sys.stderr.write('x' * 1024 * 1024)
        sys.stderr.flush()
        send({'id': request_id, 'result': True})
    elif method == 'shutdown':
        send({'id': request_id, 'result': None})
    elif method == 'exit':
        break
    elif method and request_id is not None:
        send({'id': request_id, 'error': {'code': -32601, 'message': 'Fixture unknown request'}})
