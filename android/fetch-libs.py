#!/usr/bin/env python3
"""Downloads ExoPlayer (Media3) and what it needs at run time into android/libs, for nsh.json's
"android.libraries": a small Maven resolver (the highest version asked for wins), no Gradle."""
import os, re, sys, urllib.request, xml.etree.ElementTree as ET

REPOS = ["https://dl.google.com/android/maven2", "https://repo1.maven.org/maven2"]
ROOTS = [("androidx.media3", "media3-exoplayer", "1.11.1"), ("androidx.media3", "media3-exoplayer-hls", "1.11.1")]
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "libs")
NS = "{http://maven.apache.org/POM/4.0.0}"

def fetch(path):
    for repo in REPOS:
        try:
            with urllib.request.urlopen(f"{repo}/{path}") as response:
                return response.read()
        except Exception:
            continue
    return None

def pom(group, artifact, version):
    data = fetch(f"{group.replace('.', '/')}/{artifact}/{version}/{artifact}-{version}.pom")
    return ET.fromstring(data) if data else None

def text(node, name):
    found = node.find(NS + name)
    return found.text.strip() if found is not None and found.text else None

def version_key(v):
    return [int(x) if x.isdigit() else 0 for x in re.split(r"[.\-]", v)]

wanted = {}
queue = list(ROOTS)
while queue:
    group, artifact, version = queue.pop()
    key = (group, artifact)
    if key in wanted and version_key(wanted[key]) >= version_key(version):
        continue
    wanted[key] = version
    root = pom(group, artifact, version)
    if root is None:
        print("no pom", group, artifact, version); continue
    props = {"project.version": version}
    for p in (root.find(NS + "properties") or []):
        props[p.tag.replace(NS, "")] = (p.text or "").strip()
    deps = root.find(NS + "dependencies")
    for dep in (deps if deps is not None else []):
        scope = text(dep, "scope") or "compile"
        if scope not in ("compile", "runtime") or text(dep, "optional") == "true":
            continue
        g, a, v = text(dep, "groupId"), text(dep, "artifactId"), text(dep, "version")
        if not v:
            continue
        v = re.sub(r"\$\{([^}]+)\}", lambda m: props.get(m.group(1), m.group(0)), v).strip("[]")
        if "," in v:
            v = v.split(",")[0]
        queue.append((g, a, v))

os.makedirs(OUT, exist_ok=True)
for (group, artifact), version in sorted(wanted.items()):
    base = f"{group.replace('.', '/')}/{artifact}/{version}/{artifact}-{version}"
    for ext in ("aar", "jar"):
        name = f"{artifact}-{version}.{ext}"
        if os.path.exists(os.path.join(OUT, name)):
            print("have", name); break
        data = fetch(f"{base}.{ext}")
        if data:
            open(os.path.join(OUT, name), "wb").write(data)
            print("got", name, len(data)); break
    else:
        print("NOTHING for", group, artifact, version)
