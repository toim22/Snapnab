#!/usr/bin/env python3
"""Snapnab translations (gettext format).

    python scripts/i18n.py extract    writes lang/snapnab.pot with the strings marked in the code
    python scripts/i18n.py update     adds the new .pot strings to lang/*.po (and marks the ones that were removed)
    python scripts/i18n.py check      checks the catalogs: missing strings and %d/%s markers that differ from the original

The strings in the code are Brazilian Portuguese. Mark them with Tr(L"...") where the string is shown or with
TR_NOOP(L"...") in static tables (which call Tr() when showing it). The names of the built-in stamps
(src/editor/Stamps.cpp) are extracted too, one by one. The msgid is the literal's text as it is in the code (the \\n,
\\t, \\" and \\\\ escapes are the same as in the .po).
"""
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LANG_DIR = os.path.join(ROOT, 'lang')
POT = os.path.join(LANG_DIR, 'snapnab.pot')

CALL = re.compile(r'\b(Tr|TR_NOOP)\(\s*((?:L"(?:[^"\\]|\\.)*"\s*)+)\)')
LIT = re.compile(r'L"((?:[^"\\]|\\.)*)"')
SPEC = re.compile(r'%(?:\d+\$)?[-+ #0]*(?:\d+|\*)?(?:\.\d+)?(?:hh|h|ll|l|z|j|t|I64|I32|L)?([diouxXeEfgGaAcspSn%])')


def strip_comments(src):
    # replaces comments with spaces of the same length (keeps the line numbers)
    out, i, n = [], 0, len(src)
    while i < n:
        if src.startswith('//', i):
            j = src.find('\n', i)
            j = n if j < 0 else j
            out.append(' ' * (j - i)); i = j
        elif src.startswith('/*', i):
            j = src.find('*/', i + 2)
            j = n if j < 0 else j + 2
            out.append(re.sub(r'[^\n]', ' ', src[i:j])); i = j
        elif src[i] == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == '\\' else 1
            out.append(src[i:j + 1]); i = j + 1
        elif src[i] == "'":
            j = i + 1
            while j < n and src[j] != "'":
                j += 2 if src[j] == '\\' else 1
            out.append(src[i:j + 1]); i = j + 1
        else:
            out.append(src[i]); i += 1
    return ''.join(out)


def extract():
    entries = {}            # msgid -> [references]
    order = []
    def add(msgid, ref):
        if msgid not in entries:
            entries[msgid] = []
            order.append(msgid)
        entries[msgid].append(ref)
    for dp, dn, fn in os.walk(os.path.join(ROOT, 'src')):
        for f in sorted(fn):
            if not f.endswith(('.cpp', '.h')):
                continue
            path = os.path.join(dp, f)
            rel = os.path.relpath(path, ROOT).replace('\\', '/')
            src = strip_comments(open(path, encoding='utf-8').read())
            for m in CALL.finditer(src):
                msgid = ''.join(LIT.findall(m.group(2)))
                if msgid:
                    add(msgid, f'{rel}:{src.count(chr(10), 0, m.start()) + 1}')
            if f == 'Stamps.cpp':          # names of the built-in stamps: "emoji name|emoji name|…"
                start = src.find('kStock[]')
                end = src.find('\n};', start)
                for m in re.finditer(r'TR_NOOP\(L"[^"]*"\),\s*((?:L"(?:[^"\\]|\\.)*"\s*)+)\}', src[start:end]):
                    line = src.count('\n', 0, start + m.start()) + 1
                    for item in ''.join(LIT.findall(m.group(1))).split('|'):
                        if ' ' in item:
                            add(item.split(' ', 1)[1], f'{rel}:{line}')
    return order, entries


def po_quote(s):
    # the text is already escaped as in C (\n, \", \\); breaks long lines after each \n
    parts = re.split(r'(?<=\\n)', s)
    parts = [p for p in parts if p]
    if len(parts) <= 1:
        return f'"{s}"'
    return '""\n' + '\n'.join(f'"{p}"' for p in parts)


def header(lang=None):
    h = ['# Snapnab — interface translations.', '# Copyright © 2026 Antonio Rezende. GPL-3.0.']
    if lang:
        h.append(f'# Language: {lang}.')
    h += ['#', 'msgid ""', 'msgstr ""', '"Project-Id-Version: Snapnab\\n"', '"MIME-Version: 1.0\\n"',
          '"Content-Type: text/plain; charset=UTF-8\\n"', '"Content-Transfer-Encoding: 8bit\\n"']
    if lang:
        h.append(f'"Language: {lang}\\n"')
    return '\n'.join(h) + '\n'


def write_pot():
    order, entries = extract()
    os.makedirs(LANG_DIR, exist_ok=True)
    out = [header()]
    for msgid in order:
        refs = entries[msgid]
        block = ['#: ' + ' '.join(refs[:4]) + (' …' if len(refs) > 4 else '')]
        if SPEC.search(msgid.replace('%%', '')):
            block.append('#, c-format')
        block.append('msgid ' + po_quote(msgid))
        block.append('msgstr ""')
        out.append('\n'.join(block) + '\n')
    open(POT, 'w', encoding='utf-8', newline='\n').write('\n'.join(out))
    print(f'{POT}: {len(order)} strings')


def parse_po(path):
    """returns (header, [(comments, msgid, msgstr, fuzzy)]) with the strings still escaped"""
    text = open(path, encoding='utf-8-sig').read()
    blocks = re.split(r'\n\s*\n', text.strip('\n'))
    head, items = '', []
    for b in blocks:
        comments, field, cur = [], None, {'msgid': '', 'msgstr': ''}
        for line in b.split('\n'):
            line = line.strip()
            if line.startswith('#~'):
                field = None; continue
            if line.startswith('#'):
                comments.append(line); continue
            m = re.match(r'(msgid|msgstr)\s+"(.*)"$', line)
            if m:
                field = m.group(1); cur[field] = m.group(2); continue
            m = re.match(r'"(.*)"$', line)
            if m and field:
                cur[field] += m.group(1)
        if 'msgid' in b and cur['msgid'] == '' and not items and not head:
            head = b
            continue
        if cur['msgid']:
            items.append((comments, cur['msgid'], cur['msgstr'], any('fuzzy' in c for c in comments if c.startswith('#,'))))
    return head, items


def specs(s):
    # sequence of conversions (%1$s counts at the given position)
    found = []
    for m in SPEC.finditer(s):
        if m.group(1) == '%':
            continue
        pos = re.match(r'%(\d+)\$', m.group(0))
        found.append((int(pos.group(1)) if pos else None, m.group(1).lower().replace('i', 'd')))
    if any(p for p, _ in found):
        return [t for _, t in sorted(found, key=lambda x: x[0] or 0)]
    return [t for _, t in found]


def update():
    order, entries = extract()
    for f in sorted(os.listdir(LANG_DIR)):
        if not f.endswith('.po'):
            continue
        path = os.path.join(LANG_DIR, f)
        head, items = parse_po(path)
        have = {msgid: (msgstr, fuzzy) for _, msgid, msgstr, fuzzy in items}
        out = [head.rstrip('\n') + '\n']
        for msgid in order:
            msgstr, fuzzy = have.get(msgid, ('', False))
            block = ['#: ' + ' '.join(entries[msgid][:4])]
            flags = [x for x in (('fuzzy' if fuzzy else None), ('c-format' if SPEC.search(msgid.replace('%%', '')) else None)) if x]
            if flags:
                block.append('#, ' + ', '.join(flags))
            block += ['msgid ' + po_quote(msgid), 'msgstr ' + po_quote(msgstr)]
            out.append('\n'.join(block) + '\n')
        gone = [i for i in items if i[1] not in entries and i[2]]
        for _, msgid, msgstr, _ in gone:
            out.append(f'#~ msgid "{msgid}"\n#~ msgstr "{msgstr}"\n')
        open(path, 'w', encoding='utf-8', newline='\n').write('\n'.join(out))
        print(f'{f}: {sum(1 for m in order if have.get(m, ("",))[0])}/{len(order)} translated, {len(gone)} obsolete')


def check():
    order, _ = extract()
    ok = True
    for f in sorted(os.listdir(LANG_DIR)):
        if not f.endswith('.po'):
            continue
        _, items = parse_po(os.path.join(LANG_DIR, f))
        have = {msgid: msgstr for _, msgid, msgstr, fuzzy in items if msgstr and not fuzzy}
        missing = [m for m in order if m not in have]
        bad = [m for m in order if m in have and specs(m) != specs(have[m])]
        print(f'{f}: {len(order) - len(missing)}/{len(order)} translated' + (f', {len(bad)} with different markers' if bad else ''))
        for m in bad[:20]:
            print(f'   markers: "{m}"  →  "{have[m]}"')
        ok &= not bad
    return ok


if __name__ == '__main__':
    cmd = sys.argv[1] if len(sys.argv) > 1 else 'check'
    if cmd == 'extract':
        write_pot()
    elif cmd == 'update':
        write_pot(); update()
    elif cmd == 'check':
        sys.exit(0 if check() else 1)
    else:
        print(__doc__); sys.exit(2)
