#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Export the source-owned settings guide; --check is the source CI gate."""

import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REGISTRY = Path('crates/ui/src/settings/settings_rows.rs')
HELP = Path('docs/guide-data/settings-help.json')
OUTPUT = Path('docs/guide-data/settings.json')
PATTERN = re.compile(
    r'\b(?:row|block)\(\s*Page::(\w+),\s*"([^"\\]*(?:\\.[^"\\]*)*)",'
    r'\s*"([^"\\]*(?:\\.[^"\\]*)*)",\s*"([^"\\]*(?:\\.[^"\\]*)*)",'
    r'\s*(&\[[^\]]*\]|KEYMAP)'
)


def text(value):
    return isinstance(value, str) and bool(value.strip())


def catalog(source):
    """Fail closed if a registry row no longer follows the supported source format."""
    marker = 'pub(crate) static ROWS: &[Entry] = &['
    if marker not in source:
        raise ValueError('Settings ROWS registry not found; update the exporter.')
    table = source.split(marker, 1)[1].split('#[cfg(test)]', 1)[0]
    matches = list(PATTERN.finditer(table))
    expected = len(re.findall(r'\b(?:row|block)\s*\(', table))
    if not matches or len(matches) != expected:
        raise ValueError('A settings row could not be parsed; update the exporter before publishing.')
    rows = []
    for match in matches:
        page, section, name, note, keys = match.groups()
        section, name, note = [json.loads('"' + value + '"') for value in (section, name, note)]
        note = re.sub(r'\s*\(\[docs/[^)]*\)', '', note)
        saved_keys = re.findall(r'"([^"]+)"', keys) if keys != 'KEYMAP' else ['keymap.json']
        entry_id = f'{page}/{section}/' + (','.join(saved_keys) or name)
        fingerprint = hashlib.sha256(json.dumps(
            [page, section, name, note, saved_keys], ensure_ascii=False).encode()).hexdigest()
        rows.append(dict(id=entry_id, page=page, section=section, name=name, note=note,
                         keys=saved_keys, sourceFingerprint=fingerprint))
    if len({row['id'] for row in rows}) != len(rows):
        raise ValueError('Duplicate setting IDs.')
    return rows


def export(source, authored, root=None):
    if not isinstance(authored, dict):
        raise ValueError('Authored settings help must be an object keyed by setting ID.')
    rows = catalog(source)
    unknown = set(authored) - {row['id'] for row in rows}
    if unknown:
        raise ValueError('Help IDs absent from this registry: ' + ', '.join(sorted(unknown)))
    for row in rows:
        entry_id = row['id']
        help_entry = authored.get(entry_id)
        if not isinstance(help_entry, dict) or help_entry.get('sourceFingerprint') != row['sourceFingerprint']:
            raise ValueError(f'Missing or stale help: {entry_id}. Review it in the DAW repository.')
        for key in ('description', 'whenToChange'):
            if not text(help_entry.get(key)):
                raise ValueError(f'Missing {key}: {entry_id}')
        if 'summary' in help_entry and not text(help_entry['summary']):
            raise ValueError(f'Invalid summary: {entry_id}')
        options = help_entry.get('options')
        if not isinstance(options, list) or any(not isinstance(option, dict)
                or not text(option.get('name')) or not text(option.get('description')) for option in options):
            raise ValueError(f'Invalid options: {entry_id}')
        if len({option['name'] for option in options}) != len(options):
            raise ValueError(f'Duplicate options: {entry_id}')
        references = help_entry.get('references')
        if not isinstance(references, list) or not references or not all(text(ref) for ref in references):
            raise ValueError(f'Missing implementation references: {entry_id}')
        if root is not None:
            for reference in references:
                path = (root / reference.split('::', 1)[0]).resolve()
                if not path.is_relative_to(root.resolve()) or not path.is_file():
                    raise ValueError(f'Invalid implementation reference: {entry_id}: {reference}')
        progress = help_entry.get('workInProgress', False)
        if not isinstance(progress, bool):
            raise ValueError(f'Invalid workInProgress: {entry_id}')
        row.update(note=help_entry.get('summary', row['note']), helpStatus='reviewed',
                   description=help_entry['description'], options=options,
                   whenToChange=help_entry['whenToChange'], workInProgress=progress)
    return dict(schemaVersion=1, entries=rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='Verify the committed export without writing it.')
    args = parser.parse_args()
    try:
        result = export((ROOT / REGISTRY).read_text(encoding='utf-8'),
                        json.loads((ROOT / HELP).read_text(encoding='utf-8')), ROOT)
        content = json.dumps(result, indent=2, ensure_ascii=False) + '\n'
        target = ROOT / OUTPUT
        if args.check:
            if not target.is_file() or target.read_text(encoding='utf-8') != content:
                raise ValueError('Stale settings.json. Run python scripts/settings_guide.py and commit the result.')
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(content, encoding='utf-8')
        print(f"{'Checked' if args.check else 'Exported'} {len(result['entries'])} settings with complete help.")
    except (ValueError, OSError) as error:
        parser.exit(1, f'{error}\n')


if __name__ == '__main__':
    main()
