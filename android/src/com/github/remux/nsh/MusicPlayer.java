package com.github.remux.nsh;

import android.content.Context;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;

import androidx.media3.common.AudioAttributes;
import androidx.media3.common.C;
import androidx.media3.common.MediaItem;
import androidx.media3.common.MediaMetadata;
import androidx.media3.common.PlaybackException;
import androidx.media3.common.Player;
import androidx.media3.exoplayer.ExoPlayer;

import org.json.JSONArray;
import org.json.JSONObject;

import java.util.ArrayList;
import java.util.List;

/**
 * The music player (Remux's MusicBridge): one ExoPlayer for the app with the queue in it, so the
 * next track starts by itself, without the app. Why Java: ExoPlayer is an Android library. The
 * app talks to it through its native component "remux.music" (src/music/android.rs), which also
 * reads [#snapshot] four times a second. Everything here runs on the main thread, where the
 * player lives.
 */
public final class MusicPlayer {
    private static final String TAG = "Remux";
    private static final Handler main = new Handler(Looper.getMainLooper());
    private static Context context;
    private static ExoPlayer player;
    private static volatile String state = "{}";
    private static String error = "";
    private static int failedInARow;
    private static List<Double> durationsMs = new ArrayList<>();

    private MusicPlayer() {}

    /** The app's context, for when the player is first made. */
    public static void init(Context appContext) {
        context = appContext.getApplicationContext();
    }

    /** A queue as JSON: [{url, title, artist, album, cover, durationMs}]. */
    private static List<MediaItem> items(JSONArray tracks) throws Exception {
        List<MediaItem> items = new ArrayList<>();
        for (int i = 0; i < tracks.length(); i++) {
            JSONObject track = tracks.getJSONObject(i);
            String cover = track.optString("cover", "");
            MediaMetadata metadata = new MediaMetadata.Builder()
                    .setTitle(track.optString("title"))
                    .setArtist(track.optString("artist"))
                    .setAlbumTitle(track.optString("album"))
                    .setArtworkUri(cover.isEmpty() ? null : Uri.parse(cover))
                    .build();
            items.add(new MediaItem.Builder().setUri(track.getString("url")).setMediaMetadata(metadata).build());
        }
        return items;
    }

    private static List<Double> durations(JSONArray tracks) {
        List<Double> durations = new ArrayList<>();
        for (int i = 0; i < tracks.length(); i++) {
            JSONObject track = tracks.optJSONObject(i);
            durations.add(track == null ? 0.0 : track.optDouble("durationMs", 0.0));
        }
        return durations;
    }

    private static ExoPlayer exo() {
        if (player != null) return player;
        if (context == null) return null;
        ExoPlayer created = new ExoPlayer.Builder(context)
                .setAudioAttributes(new AudioAttributes.Builder()
                        .setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).setUsage(C.USAGE_MEDIA).build(), true)
                .setHandleAudioBecomingNoisy(true)
                .setWakeMode(C.WAKE_MODE_NETWORK)
                .build();
        created.addListener(new Player.Listener() {
            @Override
            public void onPlayerError(PlaybackException e) {
                Log.e(TAG, "music error " + e.getErrorCodeName());
                error = "No stream was found for this track.";
                failedInARow += 1;
                // On to the next track, unless every one failed.
                if (failedInARow < created.getMediaItemCount() && created.hasNextMediaItem()) {
                    created.seekToNextMediaItem();
                    created.prepare();
                    created.play();
                }
                publish();
            }

            @Override
            public void onPlaybackStateChanged(int playbackState) {
                if (playbackState == Player.STATE_READY) {
                    failedInARow = 0;
                    error = "";
                }
                publish();
            }

            @Override
            public void onIsPlayingChanged(boolean isPlaying) {
                publish();
                service();
            }

            @Override
            public void onMediaItemTransition(MediaItem item, int reason) {
                service();
            }
        });
        player = created;
        tick();
        return created;
    }

    private static void tick() {
        publish();
        main.postDelayed(MusicPlayer::tick, 250);
    }

    private static void publish() {
        ExoPlayer p = player;
        if (p == null) return;
        try {
            int index = p.getCurrentMediaItemIndex();
            double said = index < durationsMs.size() ? durationsMs.get(index) : 0.0;
            double duration = said > 0 ? said : (p.getDuration() == C.TIME_UNSET ? 0.0 : p.getDuration());
            boolean ended = p.getPlaybackState() == Player.STATE_ENDED
                    || (!error.isEmpty() && p.getPlaybackState() == Player.STATE_IDLE);
            JSONObject json = new JSONObject();
            json.put("index", index);
            json.put("position", p.getCurrentPosition());
            json.put("duration", duration);
            json.put("playing", p.getPlayWhenReady() && !ended);
            json.put("buffering", p.getPlaybackState() == Player.STATE_BUFFERING && p.getPlayWhenReady());
            json.put("ended", ended);
            json.put("error", error);
            json.put("loaded", p.getMediaItemCount() > 0);
            state = json.toString();
        } catch (Exception ignored) {
        }
    }

    /** What plays, for the notification and the lock screen. */
    static final class Now {
        String title = "", artist = "", album = "", cover = "";
        long durationMs, positionMs;
        boolean playing;
    }

    /** What plays now; `null` when nothing is queued. On the main thread. */
    static Now now() {
        ExoPlayer p = player;
        if (p == null || p.getMediaItemCount() == 0) return null;
        MediaItem item = p.getCurrentMediaItem();
        Now now = new Now();
        if (item != null) {
            MediaMetadata metadata = item.mediaMetadata;
            now.title = metadata.title == null ? "" : metadata.title.toString();
            now.artist = metadata.artist == null ? "" : metadata.artist.toString();
            now.album = metadata.albumTitle == null ? "" : metadata.albumTitle.toString();
            now.cover = metadata.artworkUri == null ? "" : metadata.artworkUri.toString();
        }
        int index = p.getCurrentMediaItemIndex();
        double said = index < durationsMs.size() ? durationsMs.get(index) : 0.0;
        now.durationMs = said > 0 ? (long) said : Math.max(0, p.getDuration());
        now.positionMs = p.getCurrentPosition();
        now.playing = p.getPlayWhenReady();
        return now;
    }

    /** The service that keeps the music going off screen, told of what plays. */
    private static void service() {
        if (context != null && player != null && player.getMediaItemCount() > 0) MusicService.refresh(context);
    }

    /** The next track, as the notification's button has it (the app's queue follows). */
    static void next() {
        main.post(() -> {
            if (player == null) return;
            if (player.hasNextMediaItem()) player.seekToNextMediaItem();
            publish();
        });
    }

    /** The start of the track, or the one before when it has just begun. */
    static void previous() {
        main.post(() -> {
            if (player == null) return;
            if (player.getCurrentPosition() > 3000 || !player.hasPreviousMediaItem()) player.seekTo(0);
            else player.seekToPreviousMediaItem();
            publish();
        });
    }

    static void seek(long ms) {
        seek((double) ms);
    }

    /** How the music goes, as JSON (read from another thread). */
    public static String snapshot() {
        return state;
    }

    public static void setQueue(String tracks, int index, boolean play) {
        main.post(() -> {
            ExoPlayer p = exo();
            if (p == null) return;
            try {
                JSONArray json = new JSONArray(tracks);
                durationsMs = durations(json);
                error = "";
                failedInARow = 0;
                p.setMediaItems(items(json), index, 0L);
                p.prepare();
                p.setPlayWhenReady(play);
                service();
            } catch (Exception e) {
                Log.e(TAG, "music queue", e);
            }
            publish();
        });
    }

    /** The queue in another order, or with other tracks, while the current one plays on. */
    public static void replaceQueue(String tracks, int index) {
        main.post(() -> {
            ExoPlayer p = exo();
            if (p == null) return;
            try {
                JSONArray json = new JSONArray(tracks);
                List<MediaItem> all = items(json);
                if (p.getMediaItemCount() == 0 || index < 0 || index >= all.size()) return;
                durationsMs = durations(json);
                int current = p.getCurrentMediaItemIndex();
                if (current + 1 < p.getMediaItemCount()) p.removeMediaItems(current + 1, p.getMediaItemCount());
                if (current > 0) p.removeMediaItems(0, current);
                p.addMediaItems(0, all.subList(0, index));
                p.addMediaItems(all.subList(index + 1, all.size()));
            } catch (Exception e) {
                Log.e(TAG, "music queue", e);
            }
            publish();
        });
    }

    public static void play() {
        main.post(() -> {
            if (player == null) return;
            if (player.getPlaybackState() == Player.STATE_ENDED) player.seekTo(0, 0L);
            player.play();
            publish();
            service();
        });
    }

    public static void pause() {
        main.post(() -> {
            if (player != null) player.pause();
            publish();
            service();
        });
    }

    public static void seek(double ms) {
        main.post(() -> {
            if (player != null) player.seekTo((long) ms);
            publish();
        });
    }

    public static void skipTo(int index) {
        main.post(() -> {
            if (player == null) return;
            error = "";
            failedInARow = 0;
            player.seekTo(index, 0L);
            player.prepare();
            player.play();
            publish();
        });
    }

    /** 0 off, 1 all, 2 one. */
    public static void repeat(int mode) {
        main.post(() -> {
            if (player == null) return;
            player.setRepeatMode(mode == 1 ? Player.REPEAT_MODE_ALL : mode == 2 ? Player.REPEAT_MODE_ONE : Player.REPEAT_MODE_OFF);
        });
    }

    public static void volume(double volume) {
        main.post(() -> {
            if (player != null) player.setVolume((float) volume);
        });
    }

    public static void close() {
        main.post(() -> {
            if (player != null) {
                player.stop();
                player.clearMediaItems();
            }
            if (context != null) MusicService.end(context);
            error = "";
            durationsMs = new ArrayList<>();
            publish();
        });
    }
}
