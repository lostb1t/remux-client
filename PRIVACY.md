# Privacy policy for Remux

Remux is an app for watching what is on your own media server (Remux or Jellyfin). This page
says what it does with your data. The short version: it does not collect any, and it sends
nothing to the people who make it.

*Last updated: 7 November 2026.*

## What the app talks to

- **Your server.** The app connects to the server whose address you enter, and to nothing else
  that we run. What you watch, your library, your watch progress and your account live on that
  server, which you or its owner control. How it handles that data is up to its owner.
- **Posters from TMDB.** For some titles the app can ask [The Movie Database](https://www.themoviedb.org/)
  for a poster without text. This asks for the poster of a title by its public TMDB number, and
  does not include your account or anything else about you. It only happens in builds that
  have a TMDB key.

## What stays on your device

The address of your server, the login (a token, not your password) of the accounts you signed
in with, your settings, and the result of the last test of the connection to your server are kept
on your device only. Signing out of an account removes its login. Removing the app removes it
all.

## What the app does not do

- It has no analytics, no advertising, and no crash reporting.
- It does not sell or share data, and it does not send data to us.
- It does not track you across apps or websites.

## Permissions

- **Internet**, to reach your server.
- **The local network**, to find cast devices (such as a Chromecast) near you.

## Things you can choose to do

- **Send the log.** In the settings you can send the log of the app to your own server, to help
  find a problem. It is only sent when you ask for it, and it goes to your server, not to us.
- **Test the connection.** The app measures how fast the line to your server is, by downloading
  test data from it. That data comes from your server and is thrown away.

## Children

The app is not made for children and does not ask for anything about them.

## Changes

If this changes, the new version is put here, with a new date.

## Questions

Open an [issue](https://github.com/lostb1t/remux-client-releases/issues) on this repository.
