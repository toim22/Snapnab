#!/usr/bin/env python3
"""Traduções do Snapnab (formato gettext).

    python scripts/i18n.py extract    gera lang/snapnab.pot com os textos marcados no código
    python scripts/i18n.py update     acrescenta aos lang/*.po os textos novos do .pot (e marca os que saíram)
    python scripts/i18n.py check      confere os catálogos: textos faltando e marcadores %d/%s diferentes do original

Os textos do código são o português do Brasil. Marcar com Tr(L"...") onde o texto aparece ou com TR_NOOP(L"...") em
tabelas estáticas (que chamam Tr() ao mostrar). Os nomes dos carimbos de fábrica (src/editor/Stamps.cpp) também
entram, um a um. O msgid é o texto do literal como está no código (os escapes \\n, \\t, \\" e \\\\ são os mesmos do .po).
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
    # troca comentários por espaços do mesmo tamanho (mantém os números de linha)
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
    entries = {}            # msgid -> [referências]
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
            if f == 'Stamps.cpp':          # nomes dos carimbos de fábrica: "emoji nome|emoji nome|…"
                start = src.find('kStock[]')
                end = src.find('\n};', start)
                for m in re.finditer(r'TR_NOOP\(L"[^"]*"\),\s*((?:L"(?:[^"\\]|\\.)*"\s*)+)\}', src[start:end]):
                    line = src.count('\n', 0, start + m.start()) + 1
                    for item in ''.join(LIT.findall(m.group(1))).split('|'):
                        if ' ' in item:
                            add(item.split(' ', 1)[1], f'{rel}:{line}')
    return order, entries


def po_quote(s):
    # o texto já está escapado como no C (\n, \", \\); quebra as linhas longas depois de cada \n
    parts = re.split(r'(?<=\\n)', s)
    parts = [p for p in parts if p]
    if len(parts) <= 1:
        return f'"{s}"'
    return '""\n' + '\n'.join(f'"{p}"' for p in parts)


def header(lang=None):
    h = ['# Snapnab — traduções da interface.', '# Copyright © 2026 Antonio Rezende. GPL-3.0.']
    if lang:
        h.append(f'# Idioma: {lang}.')
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
    print(f'{POT}: {len(order)} textos')


def parse_po(path):
    """devolve (cabeçalho, [(comentários, msgid, msgstr, fuzzy)]) com os textos ainda escapados"""
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
    # sequência de conversões (%1$s conta na posição indicada)
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
        print(f'{f}: {sum(1 for m in order if have.get(m, ("",))[0])}/{len(order)} traduzidos, {len(gone)} obsoletos')


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
        print(f'{f}: {len(order) - len(missing)}/{len(order)} traduzidos' + (f', {len(bad)} com marcadores diferentes' if bad else ''))
        for m in bad[:20]:
            print(f'   marcadores: "{m}"  →  "{have[m]}"')
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
