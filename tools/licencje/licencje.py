import json, re, sys, os

meta = json.load(open(os.path.join(os.environ['TEMP'], 'szklo_meta.json'), encoding='utf-8-sig'))
pk = {p['id']: p for p in meta['packages']}
res = {n['id']: n for n in meta['resolve']['nodes']}
ws = set(meta['workspace_members'])

TARGETS = {
    'pc': dict(os='windows', family='windows', arch='x86_64', env='msvc', vendor='pc'),
    'android': dict(os='android', family='unix', arch='aarch64', env='', vendor='unknown'),
}


def ev(expr, t):
    expr = expr.strip()
    if expr.startswith('cfg(') and expr.endswith(')'):
        return ev(expr[4:-1], t)
    if not expr.startswith(('all(', 'any(', 'not(')) and '(' not in expr and '=' not in expr:
        if expr in ('windows', 'unix'):
            return t['family'] == expr
        return False
    m = re.match(r'^(all|any|not)\((.*)\)$', expr, re.S)
    if m:
        args, depth, cur = [], 0, ''
        for ch in m.group(2):
            if ch == '(':
                depth += 1
            if ch == ')':
                depth -= 1
            if ch == ',' and depth == 0:
                args.append(cur); cur = ''
            else:
                cur += ch
        if cur.strip():
            args.append(cur)
        vals = [ev(a, t) for a in args]
        return {'all': all(vals), 'any': any(vals), 'not': not vals[0] if vals else True}[m.group(1)]
    m = re.match(r'^(\w+)\s*=\s*"([^"]*)"$', expr)
    if m:
        k, v = m.groups()
        key = {'target_os': 'os', 'target_family': 'family', 'target_arch': 'arch', 'target_env': 'env', 'target_vendor': 'vendor'}.get(k)
        return key is not None and t[key] == v
    if not expr.startswith('cfg('):  # triple
        return expr.startswith(t['arch']) and t['os'] in expr
    return False


def closure(root_name, t):
    root = next(i for i in ws if pk[i]['name'] == root_name)
    seen, stack = set(), [root]
    while stack:
        i = stack.pop()
        if i in seen:
            continue
        seen.add(i)
        for d in res[i]['deps']:
            ok = False
            for k in d['dep_kinds']:
                if k['kind'] in ('dev', 'build'):
                    continue
                if k['target'] is None or ev(k['target'], t):
                    ok = True
            if ok:
                stack.append(d['pkg'])
    return sorted((pk[i] for i in seen if i not in ws), key=lambda p: p['name'].lower())


def fmt(p):
    a = ', '.join(x.split('<')[0].strip() for x in (p.get('authors') or [])[:3])
    lic = p.get('license') or ('plik licencji: ' + p['license_file'] if p.get('license_file') else '?')
    repo = p.get('repository') or p.get('homepage') or ''
    return f"{p['name']} {p['version']} — {lic}" + (f" — {a}" if a else '') + (f" — {repo}" if repo else '')


out = {}
for key, root in (('pc', 'szklo-app'), ('android', 'szklo-jni')):
    lst = closure(root, TARGETS[key])
    out[key] = [fmt(p) for p in lst]
    unknown = [p['name'] for p in lst if not p.get('license')]
    print(key, len(lst), 'bez pola license:', unknown, file=sys.stderr)
# wtyczka VST (Equalizer APO) – osobno
for key, root in (('plugin', 'szklo-vst2'),):
    try:
        out[key] = [fmt(p) for p in closure(root, TARGETS['pc'])]
    except StopIteration:
        print('brak', root, [pk[i]['name'] for i in ws], file=sys.stderr)
json.dump(out, open(os.path.join(os.environ['TEMP'], 'szklo_lic.json'), 'w', encoding='utf-8'), ensure_ascii=False, indent=1)


