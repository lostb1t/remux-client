package com.github.remux.nsh;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.media.MediaMetadata;
import android.media.session.MediaSession;
import android.media.session.PlaybackState;
import android.os.Build;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.util.Log;

import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;

/**
 * Keeps the music going while the app is not on screen, and shows it where Android shows what
 * plays: the notification with its buttons, the lock screen, a headset's keys. Why Java: a
 * service and a media session are Android's, and the app has no other way to them. It runs while
 * [MusicPlayer] has a queue, and goes when the queue is closed. Android's own MediaSession and
 * notification, not media3-session, so the app needs no other library.
 */
public final class MusicService extends Service {
    private static final String TAG = "Remux";
    private static final String CHANNEL = "music";
    private static final int ID = 1;
    private static final Handler main = new Handler(Looper.getMainLooper());
    private static MusicService running;

    private MediaSession session;
    private String coverUrl = "";
    private Bitmap cover;

    /** Starts the service, or tells it that what plays changed. From the main thread. */
    static void refresh(Context context) {
        if (running != null) {
            running.update();
            return;
        }
        Intent intent = new Intent(context, MusicService.class);
        try {
            if (Build.VERSION.SDK_INT >= 26) context.startForegroundService(intent);
            else context.startService(intent);
        } catch (Exception e) {
            // Android may refuse it from the background (12 and later); the next press in the app
            // starts it.
            Log.w(TAG, "music service: " + e);
        }
    }

    /** Stops the service: nothing is queued. */
    static void end(Context context) {
        if (running != null) {
            running.stopForeground(true);
            running.stopSelf();
        }
    }

    @Override
    public void onCreate() {
        super.onCreate();
        running = this;
        NotificationManager manager = getSystemService(NotificationManager.class);
        if (Build.VERSION.SDK_INT >= 26 && manager.getNotificationChannel(CHANNEL) == null) {
            NotificationChannel channel = new NotificationChannel(CHANNEL, "Music", NotificationManager.IMPORTANCE_LOW);
            channel.setShowBadge(false);
            manager.createNotificationChannel(channel);
        }
        session = new MediaSession(this, "Remux");
        session.setCallback(new MediaSession.Callback() {
            @Override public void onPlay() { MusicPlayer.play(); }
            @Override public void onPause() { MusicPlayer.pause(); }
            @Override public void onSkipToNext() { MusicPlayer.next(); }
            @Override public void onSkipToPrevious() { MusicPlayer.previous(); }
            @Override public void onSeekTo(long ms) { MusicPlayer.seek(ms); }
            @Override public void onStop() { MusicPlayer.pause(); }
        });
        session.setActive(true);
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        String action = intent == null ? null : intent.getAction();
        if ("play".equals(action)) MusicPlayer.play();
        else if ("pause".equals(action)) MusicPlayer.pause();
        else if ("next".equals(action)) MusicPlayer.next();
        else if ("previous".equals(action)) MusicPlayer.previous();
        update();
        return START_NOT_STICKY;
    }

    @Override
    public void onDestroy() {
        running = null;
        if (session != null) session.release();
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    private PendingIntent button(String action, int code) {
        Intent intent = new Intent(this, MusicService.class).setAction(action);
        return PendingIntent.getService(this, code, intent, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
    }

    /** The notification and the session, as the player is now. */
    private void update() {
        MusicPlayer.Now now = MusicPlayer.now();
        if (now == null) return;
        if (!now.cover.equals(coverUrl)) {
            coverUrl = now.cover;
            cover = null;
            loadCover(now.cover);
        }
        MediaMetadata.Builder metadata = new MediaMetadata.Builder()
                .putString(MediaMetadata.METADATA_KEY_TITLE, now.title)
                .putString(MediaMetadata.METADATA_KEY_ARTIST, now.artist)
                .putString(MediaMetadata.METADATA_KEY_ALBUM, now.album)
                .putLong(MediaMetadata.METADATA_KEY_DURATION, now.durationMs);
        if (cover != null) metadata.putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, cover);
        session.setMetadata(metadata.build());
        session.setPlaybackState(new PlaybackState.Builder()
                .setActions(PlaybackState.ACTION_PLAY | PlaybackState.ACTION_PAUSE | PlaybackState.ACTION_PLAY_PAUSE
                        | PlaybackState.ACTION_SKIP_TO_NEXT | PlaybackState.ACTION_SKIP_TO_PREVIOUS | PlaybackState.ACTION_SEEK_TO
                        | PlaybackState.ACTION_STOP)
                .setState(now.playing ? PlaybackState.STATE_PLAYING : PlaybackState.STATE_PAUSED, now.positionMs, now.playing ? 1f : 0f)
                .build());

        Notification.Builder builder = Build.VERSION.SDK_INT >= 26 ? new Notification.Builder(this, CHANNEL) : new Notification.Builder(this);
        Intent open = getPackageManager().getLaunchIntentForPackage(getPackageName());
        builder.setSmallIcon(android.R.drawable.ic_media_play)
                .setContentTitle(now.title)
                .setContentText(now.artist)
                .setLargeIcon(cover)
                .setVisibility(Notification.VISIBILITY_PUBLIC)
                .setOngoing(now.playing)
                .setShowWhen(false)
                .addAction(new Notification.Action.Builder(android.graphics.drawable.Icon.createWithResource(this, android.R.drawable.ic_media_previous), "Previous", button("previous", 1)).build())
                .addAction(now.playing
                        ? new Notification.Action.Builder(android.graphics.drawable.Icon.createWithResource(this, android.R.drawable.ic_media_pause), "Pause", button("pause", 2)).build()
                        : new Notification.Action.Builder(android.graphics.drawable.Icon.createWithResource(this, android.R.drawable.ic_media_play), "Play", button("play", 3)).build())
                .addAction(new Notification.Action.Builder(android.graphics.drawable.Icon.createWithResource(this, android.R.drawable.ic_media_next), "Next", button("next", 4)).build())
                .setStyle(new Notification.MediaStyle().setMediaSession(session.getSessionToken()).setShowActionsInCompactView(0, 1, 2));
        if (open != null) {
            builder.setContentIntent(PendingIntent.getActivity(this, 0, open, PendingIntent.FLAG_IMMUTABLE));
        }
        Notification notification = builder.build();
        try {
            if (Build.VERSION.SDK_INT >= 29) startForeground(ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
            else startForeground(ID, notification);
        } catch (Exception e) {
            Log.w(TAG, "music service foreground: " + e);
        }
    }

    /** The cover, for the notification and the lock screen, fetched off the main thread. */
    private void loadCover(String url) {
        if (url.isEmpty()) return;
        new Thread(() -> {
            try {
                HttpURLConnection connection = (HttpURLConnection) new URL(url).openConnection();
                connection.setConnectTimeout(10000);
                connection.setReadTimeout(10000);
                try (InputStream in = connection.getInputStream()) {
                    Bitmap bitmap = BitmapFactory.decodeStream(in);
                    main.post(() -> {
                        if (url.equals(coverUrl) && running == this) {
                            cover = bitmap;
                            update();
                        }
                    });
                }
            } catch (Exception e) {
                Log.w(TAG, "music cover: " + e);
            }
        }).start();
    }
}
