#!/usr/bin/env python3
"""Downloads ExoPlayer (Media3) and what it needs at run time into android/libs, for nsh.json's
"android.libraries": a small Maven resolver (the highest version asked for wins), no Gradle."""
import os, re, sys, time, urllib.error, urllib.request, xml.etree.ElementTree as ET

REPOS = ["https://dl.google.com/android/maven2", "https://repo1.maven.org/maven2"]
ROOTS = [("androidx.media3", "media3-exoplayer", "1.11.1"), ("androidx.media3", "media3-exoplayer-hls", "1.11.1")]
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "libs")
NS = "{http://maven.apache.org/POM/4.0.0}"

def fetch(path):
    """The file from the first repository that has it. Not there (404) goes on to the next one; a
    network error is tried again a few times, as CI's network has a hiccup now and then (a jar
    that went missing that way failed the build only later, at javac)."""
    for repo in REPOS:
        for attempt in range(4):
            try:
                with urllib.request.urlopen(f"{repo}/{path}", timeout=60) as response:
                    return response.read()
            except urllib.error.HTTPError as error:
                if error.code == 404:
                    break
                print(f"retry {path}: HTTP {error.code}", file=sys.stderr)
            except Exception as error:
                print(f"retry {path}: {error}", file=sys.stderr)
            time.sleep(2 * (attempt + 1))
    return None

missing = []

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
        print("no pom", group, artifact, version)
        missing.append(f"{group}:{artifact}:{version} (pom)")
        continue
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
        missing.append(f"{group}:{artifact}:{version}")

# Without all of them the app's Java does not build (or, worse, builds without what it needs at
# run time): say so here.
if missing:
    sys.exit("could not fetch: " + ", ".join(missing))
