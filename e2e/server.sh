#!/bin/bash
# A Jellyfin to test against, made from nothing: a container, signed up with a user, and a handful
# of (empty) libraries, which is all the pages of the app that the flows visit need.
#
#   e2e/server.sh start   starts it and prints the lines to put in the environment:
#                           REMUX_TEST_SERVER (as the emulator reaches it), _TOKEN and _USER
#   e2e/server.sh stop    removes it
#
# Environment: E2E_PORT (default 18096), E2E_IMAGE (default jellyfin/jellyfin:10.11.6).
set -euo pipefail
name=remux-e2e-jellyfin
port="${E2E_PORT:-18096}"
image="${E2E_IMAGE:-jellyfin/jellyfin:10.11.6}"
base="http://localhost:$port"
auth='MediaBrowser Client="remux-e2e", Device="e2e", DeviceId="remux-e2e", Version="1"'

api() { # method path [json]
  curl -fsS -X "$1" "$base$2" -H "Content-Type: application/json" -H "X-Emby-Authorization: $auth${TOKEN:+, Token=\"$TOKEN\"}" ${3:+-d "$3"}
}

case "${1:-start}" in
  stop)
    docker rm -f "$name" >/dev/null 2>&1 || true
    ;;
  start)
    docker rm -f "$name" >/dev/null 2>&1 || true
    docker run -d --name "$name" -p "$port:8096" "$image" >/dev/null
    TOKEN=""
    # It answers the public info before it is ready for the wizard (503 until then).
    for _ in $(seq 1 90); do
      api GET /Startup/Configuration >/dev/null 2>&1 && break
      sleep 2
    done
    api GET /Startup/Configuration >/dev/null
    # The start-up wizard: a user, and done.
    api POST /Startup/Configuration '{"UICulture":"en-US","MetadataCountryCode":"US","PreferredMetadataLanguage":"en"}' >/dev/null
    api GET /Startup/User >/dev/null
    api POST /Startup/User '{"Name":"e2e","Password":"e2e-password"}' >/dev/null
    api POST /Startup/Complete >/dev/null

    login="$(api POST /Users/AuthenticateByName '{"Username":"e2e","Pw":"e2e-password"}')"
    TOKEN="$(python3 -c 'import json,sys;print(json.load(sys.stdin)["AccessToken"])' <<<"$login")"
    user="$(python3 -c 'import json,sys;print(json.load(sys.stdin)["User"]["Id"])' <<<"$login")"

    # Enough libraries for the Media page to have a few rows of tiles (it has three to a row on
    # a TV, and Favorites and Playlists come before them).
    i=0
    for library in Movies Shows Music "Kids Movies" Documentaries Anime Concerts "Home Videos"; do
      i=$((i + 1))
      docker exec "$name" mkdir -p "/media/lib$i"
      encoded="$(python3 -c 'import sys,urllib.parse;print(urllib.parse.quote(sys.argv[1]))' "$library")"
      type=movies; [ "$library" = Shows ] && type=tvshows; [ "$library" = Music ] && type=music
      api POST "/Library/VirtualFolders?name=$encoded&collectionType=$type&refreshLibrary=false" \
        "{\"LibraryOptions\":{\"PathInfos\":[{\"Path\":\"/media/lib$i\"}]}}" >/dev/null
    done

    echo "REMUX_TEST_SERVER=http://10.0.2.2:$port"
    echo "REMUX_TEST_TOKEN=$TOKEN"
    echo "REMUX_TEST_USER=$user"
    ;;
  *)
    echo "usage: $0 start|stop" >&2
    exit 2
    ;;
esac
