package com.github.remux.nsh;

import android.content.Context;
import android.graphics.Color;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import android.view.Gravity;
import android.view.SurfaceView;
import android.widget.FrameLayout;

import androidx.media3.common.C;
import androidx.media3.common.Format;
import androidx.media3.common.MediaItem;
import androidx.media3.common.MimeTypes;
import androidx.media3.common.PlaybackException;
import androidx.media3.common.Player;
import androidx.media3.common.TrackSelectionOverride;
import androidx.media3.common.Tracks;
import androidx.media3.common.VideoSize;
import androidx.media3.common.text.Cue;
import androidx.media3.common.text.CueGroup;
import androidx.media3.datasource.DefaultHttpDataSource;
import androidx.media3.exoplayer.DefaultRenderersFactory;
import androidx.media3.exoplayer.ExoPlayer;
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory;

import org.json.JSONArray;
import org.json.JSONObject;

import java.util.ArrayList;
import java.util.List;

/**
 * The video player of the app: ExoPlayer drawing into a surface that keeps the picture's
 * proportions (Remux's MpvBridge, which is ExoPlayer too). Why Java: ExoPlayer is an Android
 * library; NSH places this view through the app's native component "remux.exo" (src/player/exo.rs),
 * which also reads [#snapshot] to tell the app how playback goes. Subtitles are not drawn here:
 * their text goes to the app in the snapshot, which draws it.
 */
public final class ExoView extends FrameLayout {
    private static final String TAG = "Remux";
    private final Handler main = new Handler(Looper.getMainLooper());
    private final SurfaceView surface;
    private ExoPlayer player;
    private float aspect = 16f / 9f;

    private volatile String state = "{}";
    private String error = "";
    private String cues = "";
    /** Subtitles that are pictures (PGS and the like), drawn over the video. */
    private final PictureCues pictures;
    private boolean loaded;
    /** The subtitle asked for (a stream index, or -1), applied as soon as the tracks are known. */
    private long wantedSubtitle = -1;
    /** The audio track asked for (a stream index, or -1 for what the player picks). */
    private long wantedAudio = -1;
    /** The stream indices of the audio tracks of the file, in the order the file has them. */
    private List<Long> audioOrder = new ArrayList<>();
    /** The subtitles the player reads from the file itself: stream index and ExoPlayer's kind. */
    private List<Object[]> fileSubtitles = new ArrayList<>();

    public ExoView(Context context) {
        super(context);
        setBackgroundColor(Color.BLACK);
        surface = new SurfaceView(context);
        // The screen stays on while the picture shows.
        surface.setKeepScreenOn(true);
        addView(surface, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT, Gravity.CENTER));
        // Subtitles that are pictures, over the video.
        pictures = new PictureCues(context);
        addView(pictures, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT, Gravity.CENTER));
        tick();
    }

    /** The surface as large as fits, at the picture's proportions, in the middle. */
    @Override
    protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
        int width = right - left, height = bottom - top;
        int w = width, h = Math.round(width / aspect);
        if (h > height) {
            h = height;
            w = Math.round(height * aspect);
        }
        int x = (width - w) / 2, y = (height - h) / 2;
        surface.layout(x, y, x + w, y + h);
        pictures.layout(x, y, x + w, y + h);
    }

    private ExoPlayer exo() {
        if (player != null) return player;
        Context context = getContext();
        // A subtitle the server has not made yet can take longer than the default 8 seconds, and a
        // live channel can be sent on (a redirect) from http to the provider's https.
        DefaultHttpDataSource.Factory http = new DefaultHttpDataSource.Factory()
                .setConnectTimeoutMs(30_000)
                .setReadTimeoutMs(90_000)
                .setAllowCrossProtocolRedirects(true);
        ExoPlayer created = new ExoPlayer.Builder(context)
                .setRenderersFactory(new DefaultRenderersFactory(context).setEnableDecoderFallback(true))
                .setMediaSourceFactory(new DefaultMediaSourceFactory(context).setDataSourceFactory(http))
                .build();
        created.addListener(new Player.Listener() {
            @Override
            public void onPlayerError(PlaybackException e) {
                // A live stream that was left behind carries on at the live edge.
                if (e.errorCode == PlaybackException.ERROR_CODE_BEHIND_LIVE_WINDOW) {
                    created.seekToDefaultPosition();
                    created.prepare();
                    return;
                }
                for (Throwable cause = e.getCause(); cause != null; cause = cause.getCause()) {
                    Log.e(TAG, "player error " + e.getErrorCodeName() + ": " + cause);
                }
                error = e.getMessage() != null ? e.getMessage() : e.getErrorCodeName();
                publish();
            }

            @Override
            public void onVideoSizeChanged(VideoSize size) {
                if (size.height > 0) {
                    aspect = size.width * size.pixelWidthHeightRatio / size.height;
                    requestLayout();
                }
            }

            @Override
            public void onPlaybackStateChanged(int playbackState) {
                Log.i(TAG, "state " + playbackState + " at " + created.getCurrentPosition());
            }

            @Override
            public void onTracksChanged(Tracks tracks) {
                // A track the decoders cannot play is left out without an error: sound, no picture.
                for (Tracks.Group group : tracks.getGroups()) {
                    for (int i = 0; i < group.length; i++) {
                        Format f = group.getTrackFormat(i);
                        Log.i(TAG, "track type=" + group.getType() + " " + f.sampleMimeType + " codecs=" + f.codecs + " " + f.width + "x" + f.height
                                + " supported=" + group.isTrackSupported(i) + " selected=" + group.isTrackSelected(i));
                    }
                }
                applyAudio();
                applySubtitle();
            }

            @Override
            public void onCues(CueGroup group) {
                int bitmaps = 0;
                for (Cue cue : group.cues) if (cue.bitmap != null) bitmaps++;
                Log.i(TAG, "cues " + group.cues.size() + " (pictures " + bitmaps + ") at " + (player != null ? player.getCurrentPosition() : -1));
                pictures.show(group.cues);
                StringBuilder text = new StringBuilder();
                for (Cue cue : group.cues) {
                    if (cue.text == null) continue;
                    if (text.length() > 0) text.append('\n');
                    text.append(cue.text);
                }
                cues = text.toString();
                publish();
            }
        });
        created.setTrackSelectionParameters(created.getTrackSelectionParameters().buildUpon()
                .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true).build());
        created.setVideoSurfaceView(surface);
        player = created;
        return created;
    }

    /** Keeps the snapshot up to date while the view lives. */
    private void tick() {
        publish();
        main.postDelayed(this::tick, 250);
    }

    private void publish() {
        try {
            JSONObject json = new JSONObject();
            ExoPlayer p = player;
            if (p != null && loaded) {
                long duration = p.getDuration() == C.TIME_UNSET ? 0 : p.getDuration();
                json.put("position", p.getCurrentPosition());
                json.put("duration", duration);
                json.put("paused", !p.getPlayWhenReady());
                json.put("ended", p.getPlaybackState() == Player.STATE_ENDED);
                json.put("buffering", p.getPlaybackState() == Player.STATE_BUFFERING);
                json.put("ready", p.getPlaybackState() == Player.STATE_READY);
            }
            json.put("error", error);
            json.put("cues", cues);
            state = json.toString();
        } catch (Exception ignored) {
        }
    }

    /** What the app reads, from any thread: a JSON object of how playback goes. */
    public String snapshot() {
        return state;
    }

    /**
     * Plays a stream: `json` has `url`, `startMs`, `hls`, `subtitles` (objects with `index`,
     * `label`, `language` and `url`, the WebVTT the server makes), `audioOrder` (the stream indices
     * of the file's audio tracks, in order) and `fileSubtitles` (`[index, codec]` of the subtitles
     * the player reads from the file).
     */
    public void load(String json) {
        try {
            JSONObject source = new JSONObject(json);
            ExoPlayer p = exo();
            error = "";
            cues = "";
            pictures.show(new ArrayList<>());
            wantedSubtitle = -1;
            wantedAudio = -1;
            p.setTrackSelectionParameters(p.getTrackSelectionParameters().buildUpon()
                    .clearOverridesOfType(C.TRACK_TYPE_TEXT)
                    .clearOverridesOfType(C.TRACK_TYPE_AUDIO)
                    .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
                    .build());
            audioOrder = new ArrayList<>();
            JSONArray order = source.optJSONArray("audioOrder");
            for (int i = 0; order != null && i < order.length(); i++) audioOrder.add(order.getLong(i));
            fileSubtitles = new ArrayList<>();
            JSONArray inFile = source.optJSONArray("fileSubtitles");
            for (int i = 0; inFile != null && i < inFile.length(); i++) {
                JSONArray entry = inFile.getJSONArray(i);
                fileSubtitles.add(new Object[] {entry.getLong(0), subtitleKind(entry.getString(1))});
            }
            List<MediaItem.SubtitleConfiguration> configurations = new ArrayList<>();
            JSONArray subtitles = source.optJSONArray("subtitles");
            for (int i = 0; subtitles != null && i < subtitles.length(); i++) {
                JSONObject subtitle = subtitles.getJSONObject(i);
                String language = subtitle.optString("language", "");
                configurations.add(new MediaItem.SubtitleConfiguration.Builder(Uri.parse(subtitle.getString("url")))
                        .setMimeType(MimeTypes.TEXT_VTT)
                        .setId("idx:" + subtitle.getLong("index"))
                        .setLabel(subtitle.optString("label", ""))
                        .setLanguage(language.isEmpty() ? null : language)
                        .build());
            }
            // The address without its query, which has the key to the server.
            Log.i(TAG, "load " + source.getString("url").split("\\?")[0] + " hls=" + source.optBoolean("hls") + " start=" + source.optDouble("startMs", 0));
            MediaItem.Builder item = new MediaItem.Builder().setUri(source.getString("url")).setSubtitleConfigurations(configurations);
            // Said outright: the address of a playlist the server sends on has no .m3u8 in it.
            if (source.optBoolean("hls")) item.setMimeType(MimeTypes.APPLICATION_M3U8);
            p.setMediaItem(item.build(), (long) source.optDouble("startMs", 0));
            p.prepare();
            p.setPlayWhenReady(true);
            loaded = true;
            publish();
        } catch (Exception e) {
            Log.e(TAG, "load", e);
            error = String.valueOf(e.getMessage());
            publish();
        }
    }

    public void setPaused(boolean paused) {
        if (player != null) player.setPlayWhenReady(!paused);
        publish();
    }

    /** The speed it plays at (1 is as made). */
    public void setRate(double rate) {
        if (player != null) player.setPlaybackSpeed((float) Math.max(0.25, Math.min(4.0, rate)));
    }

    public void seek(double ms) {
        if (player != null) player.seekTo((long) ms);
        publish();
    }

    /** The subtitle with this stream index; a negative one hides them. */
    public void subtitle(long index) {
        wantedSubtitle = index;
        if (index < 0) {
            cues = "";
            pictures.show(new ArrayList<>());
        }
        applySubtitle();
    }

    /** The audio track with this stream index of the file; a negative one leaves it to the player. */
    public void audio(long index) {
        wantedAudio = index;
        applyAudio();
    }

    /** Stops and lets go of the player: the element is gone. */
    public void release() {
        main.removeCallbacksAndMessages(null);
        if (player != null) {
            player.release();
            player = null;
        }
        loaded = false;
    }

    /** What ExoPlayer knows a subtitle of the file by, from the codec the server names. */
    private static String subtitleKind(String codec) {
        switch (codec.toLowerCase()) {
            case "pgssub": case "pgs": case "hdmv_pgs_subtitle": return MimeTypes.APPLICATION_PGS;
            case "dvbsub": case "dvb_subtitle": return MimeTypes.APPLICATION_DVBSUBS;
            case "srt": case "subrip": return MimeTypes.APPLICATION_SUBRIP;
            case "ass": case "ssa": return MimeTypes.TEXT_SSA;
            case "vtt": case "webvtt": return MimeTypes.TEXT_VTT;
            case "mov_text": return MimeTypes.APPLICATION_TX3G;
            default: return codec;
        }
    }

    private static String trackKind(Format format) {
        return MimeTypes.APPLICATION_MEDIA3_CUES.equals(format.sampleMimeType) ? format.codecs : format.sampleMimeType;
    }

    private void applyAudio() {
        ExoPlayer p = player;
        if (p == null || wantedAudio < 0) return;
        int ordinal = audioOrder.indexOf(wantedAudio);
        if (ordinal < 0) return;
        List<Tracks.Group> audio = new ArrayList<>();
        for (Tracks.Group group : p.getCurrentTracks().getGroups()) if (group.getType() == C.TRACK_TYPE_AUDIO) audio.add(group);
        if (ordinal >= audio.size() || !audio.get(ordinal).isTrackSupported(0)) return;
        p.setTrackSelectionParameters(p.getTrackSelectionParameters().buildUpon()
                .setTrackTypeDisabled(C.TRACK_TYPE_AUDIO, false)
                .setOverrideForType(new TrackSelectionOverride(audio.get(ordinal).getMediaTrackGroup(), 0))
                .build());
    }

    /**
     * Shows the wanted subtitle once its track is listed: one added by address by its id, one of
     * the file itself by its place among the file's subtitles of that kind.
     */
    private void applySubtitle() {
        ExoPlayer p = player;
        if (p == null) return;
        if (wantedSubtitle < 0) {
            p.setTrackSelectionParameters(p.getTrackSelectionParameters().buildUpon().setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true).build());
            return;
        }
        List<Tracks.Group> text = new ArrayList<>();
        for (Tracks.Group group : p.getCurrentTracks().getGroups()) {
            if (group.getType() == C.TRACK_TYPE_TEXT && group.length > 0) text.add(group);
        }
        Object[] inFile = null;
        for (Object[] entry : fileSubtitles) if ((long) entry[0] == wantedSubtitle) inFile = entry;
        Tracks.Group found = null;
        if (inFile != null) {
            int ordinal = 0;
            for (Object[] entry : fileSubtitles) {
                if ((long) entry[0] == wantedSubtitle) break;
                if (entry[1].equals(inFile[1])) ordinal++;
            }
            int seen = 0;
            for (Tracks.Group group : text) {
                Format format = group.getTrackFormat(0);
                boolean added = format.id != null && format.id.contains("idx:");
                if (!added && inFile[1].equals(trackKind(format))) {
                    if (seen == ordinal) {
                        found = group;
                        break;
                    }
                    seen++;
                }
            }
        } else {
            for (Tracks.Group group : text) {
                String id = group.getTrackFormat(0).id;
                if (id != null && id.endsWith("idx:" + wantedSubtitle)) found = group;
            }
        }
        if (found == null) return;
        p.setTrackSelectionParameters(p.getTrackSelectionParameters().buildUpon()
                .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
                .setOverrideForType(new TrackSelectionOverride(found.getMediaTrackGroup(), 0))
                .build());
    }
}
