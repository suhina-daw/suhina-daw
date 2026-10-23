#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Export default-bound and window shortcuts; native CI owns prose coverage."""

import argparse
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
KEYMAP = 'crates/ui/src/settings/keymap.rs'
HELP = 'docs/guide-data/shortcuts-help.json'
OUTPUT = 'docs/guide-data/shortcuts.json'
PRESETS = ('Select-first', 'Draw-first', 'Mouse-first')
PLATFORMS = ('windows', 'linux', 'macos')


def table(source, name):
    match = re.search(r'const ' + name + r':.*?;\s*(\d+)\]\s*=\s*(\[.*?\]);', source, re.S)
    if not match:
        raise ValueError(f'Cannot parse {name}; update the exporter.')
    return int(match[1]), match[2]


def catalog(root=ROOT):
    source = (root / KEYMAP).read_text(encoding='utf-8')
    actions = {}
    bindings = {}
    for name in ('WINDOW_ACTIONS', 'DETAIL_ACTIONS'):
        count, body = table(source, name)
        pattern = r'\(\s*"([^"\n]+)",\s*"([^"\n]+)",\s*("[^"\n]+"|PIANO_ROLL),?\s*\)'
        rows = re.findall(pattern, body)
        if len(rows) != count:
            raise ValueError(f'Incomplete action table: {name}')
        for action_id, label, group in rows:
            if action_id in actions:
                raise ValueError('Duplicate action: ' + action_id)
            actions[action_id] = (label, 'Piano roll' if group == 'PIANO_ROLL' else json.loads(group))
    for name in ('DEFAULTS', 'DETAIL_DEFAULTS'):
        count, body = table(source, name)
        rows = re.findall(r'\(\s*"([^"\n]+)",\s*&\[([^\]]*)\]\s*\)', body)
        if len(rows) != count or body.count('(') != count:
            raise ValueError(f'Incomplete default table: {name}')
        for action_id, keys in rows:
            if action_id in bindings:
                raise ValueError('Duplicate default: ' + action_id)
            values = re.findall(r'"([^"\n]+)"', keys)
            if not values or re.sub(r'"[^"\n]+"|[,\s]', '', keys):
                raise ValueError('Invalid default keys: ' + action_id)
            bindings[action_id] = values
    # Commands with default keys are in the native session/project registries.
    for action_id in sorted(set(bindings) - set(actions)):
        found = []
        # A namespace's commands are in crates/session/src/commands/<namespace>.rs (#764 › 14).
        paths = sorted((root / 'crates/session/src/commands').glob('*.rs'))
        for path in paths + [root / 'crates/session/src/projects.rs']:
            text = path.read_text(encoding='utf-8')
            pattern = r'id:\s*"' + re.escape(action_id) + r'",\s*label:\s*"([^"\n]+)"'
            found.extend(re.findall(pattern, text))
        if len(found) != 1:
            raise ValueError('Missing or ambiguous command: ' + action_id)
        # keymap::actions() appends parameterless registry actions to Commands,
        # regardless of the command ID's namespace.
        actions[action_id] = (found[0], 'Commands')
    # The preset algorithm and modifier translation are part of the evidence fingerprint.
    evidence = source.split('#[cfg(test)]', 1)[0]
    digest = hashlib.sha256(evidence.encode()).hexdigest()
    entries = []
    for action_id, (name, category) in actions.items():
        defaults = bindings.get(action_id, [])
        presets = {}
        for preset in PRESETS:
            keys = defaults
            if preset == 'Draw-first' and action_id == 'selection.duplicate':
                keys = ['Ctrl+B']
            if preset == 'Draw-first' and action_id == 'view.browser':
                keys = ['Ctrl+Shift+B']
            presets[preset] = {os: [tokens(key, os) for key in keys
                                   if not (os == 'macos' and action_id == 'view.zoomIn' and key == 'Ctrl+Plus')]
                               for os in PLATFORMS}
        row = dict(id=action_id, name=name, category=category, bindings=presets)
        row['sourceFingerprint'] = hashlib.sha256(json.dumps([digest, row], sort_keys=True).encode()).hexdigest()
        entries.append(row)
    return entries


def tokens(key, platform):
    parts = key.split('+')
    names = {'Ctrl': 'Cmd', 'Alt': 'Option'} if platform == 'macos' else {}
    aliases = {'Comma': ',', 'Plus': '+', 'Equals': '=', 'Minus': '-', 'Up': '↑', 'Down': '↓', 'Left': '←', 'Right': '→'}
    return [names.get(part, aliases.get(part, part)) for part in parts]


def export(root=ROOT, authored=None):
    if authored is None:
        authored = json.loads((root / HELP).read_text(encoding='utf-8'))
    rows = catalog(root)
    if not isinstance(authored, dict) or set(authored) != {row['id'] for row in rows}:
        raise ValueError('Shortcut help must exactly cover default-bound and window actions.')
    for row in rows:
        help_entry = authored[row['id']]
        if not isinstance(help_entry, dict) or help_entry.get('sourceFingerprint') != row['sourceFingerprint']:
            raise ValueError('Missing or stale shortcut help: ' + row['id'])
        for field in ('description', 'context', 'whenToUse', 'platformNotes'):
            if not isinstance(help_entry.get(field), str) or not help_entry[field].strip():
                raise ValueError(f'Missing {field}: {row["id"]}')
            row[field] = help_entry[field]
        refs = help_entry.get('references')
        if not isinstance(refs, list) or not refs:
            raise ValueError('Missing references: ' + row['id'])
        for ref in refs:
            if not isinstance(ref, str) or not (root / ref).resolve().is_relative_to(root.resolve()) or not (root / ref).is_file():
                raise ValueError('Invalid reference: ' + row['id'])
        row['references'] = list(refs)
        row['helpStatus'] = 'reviewed'
    return dict(schemaVersion=1, scope='default-and-window-actions', entries=rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    content = json.dumps(export(), indent=2, ensure_ascii=False) + '\n'
    target = ROOT / OUTPUT
    if args.check:
        if not target.is_file() or target.read_text(encoding='utf-8') != content:
            parser.error('Shortcut export is stale; review help and run python scripts/shortcuts_guide.py.')
    else:
        target.write_text(content, encoding='utf-8', newline='\n')
    print(f'{len(catalog())} shortcut actions: complete help and deterministic export.')


if __name__ == '__main__':
    main()
