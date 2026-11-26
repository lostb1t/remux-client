// The web app in a browser (`nsh test tests --target web`: headless Chrome), against the test
// server, as a user: the login page, Home, the places, a title's page and back, and playing The
// Matrix (a film the test server plays). Each at a desktop's size (1920 × 1080: most people watch
// on a big screen) and a phone's (390 × 844).
//
// The server and the user are in `tests/test-server.ts` (not in git; the test engine has no
// environment of its own): CI writes it from the secrets E2E_SERVER, E2E_USERNAME and
// E2E_PASSWORD, as `export const server = "…", username = "…", password = "…";`.
import { open, test } from "nsh/test";
import { password, server, username } from "./test-server";

const playButton = "text:/^(Play|Resume)$/";

/** The sizes each test runs at. */
const sizes = [
  { name: "desktop", width: 1920, height: 1080 },
  { name: "phone", width: 390, height: 844 },
];

/** Runs `steps` on `app`; when one fails, what was on screen goes to test-results/ (CI keeps
 *  it), then the failure goes on. */
async function shown(app: Awaited<ReturnType<typeof open>>, name: string, steps: () => Promise<void>) {
  try {
    await steps();
  } catch (error) {
    await app.screenshot(`test-results/${name.replace(/[^a-z0-9]+/gi, "-")}.png`).catch(() => {});
    throw error;
  }
}

/** The app, signed in at its login page, on Home. */
async function signedIn(size: { width: number; height: number }) {
  const app = await open("..", { network: "live", width: size.width, height: size.height });
  await app.waitFor("text:Log In", { timeout: 30_000 });
  await app.type("text:Server URL", server);
  await app.type("text:Username", username);
  await app.type("text:Password", password);
  await app.press("label:Log In");
  await app.waitFor(playButton, { timeout: 60_000 });
  return app;
}

for (const size of sizes) {
  test(`signs in, and goes through the places (${size.name})`, async () => {
    const app = await signedIn(size);
    await app.press("id:tab-media");
    await app.waitFor("label:Favorites", { timeout: 15_000 });
    await app.press("id:tab-discover");
    await app.waitFor("text:Search", { timeout: 15_000 });
    await app.press("id:tab-live-tv");
    await app.waitFor("label:Channels", { timeout: 15_000 });
    await app.press("id:tab-home");
    await app.waitFor(playButton, { timeout: 15_000 });
  }, 120_000);

  test(`a title's page from the hero, and back (${size.name})`, async () => {
    const app = await signedIn(size);
    await app.press("id:hero-picture >> nth=0");
    await app.waitFor("id:page-back", { timeout: 15_000 });
    // The page's Back (the browser's leaves the app: pages have no address yet, PLAN.md).
    await app.press("id:page-back");
    await app.waitFor("id:page-back", { state: "gone", timeout: 5_000 });
    await app.waitFor(playButton);
  }, 120_000);

  test(`plays The Matrix (${size.name})`, async () => {
    const app = await signedIn(size);
    await shown(app, `matrix-${size.name}`, async () => {
      await app.press("id:tab-discover");
      await app.type("id:search", "The Matrix");
      await app.submit("id:search");
      // The film: of the Movies row (the music has The Matrix too, first at a desktop's width).
      const film = "id:results-movies >> label:/^The Matrix$/ >> nth=0";
      await app.waitFor(film, { timeout: 20_000 });
      await app.press(film);
      await app.waitFor("id:page-back", { timeout: 15_000 });
      await app.press(playButton);
      // Which version: the first, by what its video says.
      await app.waitFor("text:Version", { timeout: 30_000 });
      const version = "text:/Direct Play|Direct Stream|Transcode/ >> nth=0";
      await app.waitFor(version, { timeout: 30_000 });
      await app.press(version);
      await app.waitFor("label:Play or pause", { timeout: 60_000 });
      await app.press("label:Close");
      await app.waitFor("label:Play or pause", { state: "gone", timeout: 10_000 });
      await app.waitFor("id:page-back");
    });
  }, 180_000);
}
