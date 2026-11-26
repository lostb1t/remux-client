package com.github.remux.nsh;

import android.content.Context;
import android.media.MediaCodecInfo;
import android.media.MediaCodecList;
import android.os.Build;
import android.util.Log;
import android.view.Display;
import android.view.WindowManager;

/**
 * What the device says about HDR (Remux's `displayHdr`). Why Java: the screen's HDR types and
 * the decoders are Android's, which the app reads through its native component `remux.music`
 * (src/music/android.rs), once, as it starts.
 */
public final class DeviceInfo {
    private DeviceInfo() {}

    private static android.net.wifi.WifiManager.MulticastLock multicast;

    /**
     * Lets the app hear the network's multicast, which finding Chromecasts (mDNS) needs: Android
     * drops it for an app without this lock. Held while the app runs.
     */
    public static void multicastLock(Context context) {
        if (multicast != null) return;
        // A TV is what is cast to, not what casts.
        android.app.UiModeManager ui = (android.app.UiModeManager) context.getSystemService(Context.UI_MODE_SERVICE);
        if (ui != null && ui.getCurrentModeType() == android.content.res.Configuration.UI_MODE_TYPE_TELEVISION) return;
        try {
            android.net.wifi.WifiManager wifi = (android.net.wifi.WifiManager) context.getApplicationContext().getSystemService(Context.WIFI_SERVICE);
            multicast = wifi.createMulticastLock("remux-cast");
            multicast.setReferenceCounted(false);
            multicast.acquire();
        } catch (Throwable e) {
            Log.w("Remux", "no multicast lock: " + e);
        }
    }

    /**
     * As bits: the screen takes 1 Dolby Vision, 2 HDR10, 4 HLG, 8 HDR10+; 16 Android calls the
     * screen HDR; the decoder for HEVC does 32 Main 10, 64 with HDR10, 128 with HDR10+.
     */
    @SuppressWarnings("deprecation")
    public static int hdrBits(Context context) {
        int bits = 0;
        try {
            WindowManager windows = (WindowManager) context.getSystemService(Context.WINDOW_SERVICE);
            Display.HdrCapabilities capabilities = windows.getDefaultDisplay().getHdrCapabilities();
            int[] types = capabilities == null ? new int[0] : capabilities.getSupportedHdrTypes();
            for (int type : types) {
                if (type == Display.HdrCapabilities.HDR_TYPE_DOLBY_VISION) bits |= 1;
                else if (type == Display.HdrCapabilities.HDR_TYPE_HDR10) bits |= 2;
                else if (type == Display.HdrCapabilities.HDR_TYPE_HLG) bits |= 4;
                else if (type == Display.HdrCapabilities.HDR_TYPE_HDR10_PLUS) bits |= 8;
            }
            if (Build.VERSION.SDK_INT >= 26 && context.getResources().getConfiguration().isScreenHdr()) bits |= 16;
        } catch (Throwable e) {
            Log.w("Remux", "no HDR info of the screen: " + e);
        }
        try {
            for (MediaCodecInfo info : new MediaCodecList(MediaCodecList.REGULAR_CODECS).getCodecInfos()) {
                if (info.isEncoder()) continue;
                boolean hevc = false;
                for (String type : info.getSupportedTypes()) {
                    hevc |= type.equalsIgnoreCase("video/hevc");
                    // 256: a decoder for Dolby Vision itself (a file with no HDR10 layer under it).
                    if (type.equalsIgnoreCase("video/dolby-vision")) bits |= 256;
                }
                if (!hevc) continue;
                for (MediaCodecInfo.CodecProfileLevel level : info.getCapabilitiesForType("video/hevc").profileLevels) {
                    if (level.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10) bits |= 32;
                    else if (level.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10) bits |= 64;
                    else if (level.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10Plus) bits |= 128;
                }
            }
        } catch (Throwable e) {
            Log.w("Remux", "no HEVC info: " + e);
        }
        return bits;
    }

    /**
     * The screen and what the video decoders in hardware take (Remux's `displayInfo`), as "screen
     * width, height, H.264 width, height, HEVC width, height, VP9 width, height, AV1 width,
     * height", in pixels with the long side first; 0 where it isn't known, which for a codec means
     * the device has no decoder in hardware for it. A decoder's size is the largest of a few
     * common ones it accepts.
     */
    public static String displayInfo(Context context) {
        String screen = "0,0";
        try {
            WindowManager windows = (WindowManager) context.getSystemService(Context.WINDOW_SERVICE);
            Display.Mode mode = windows.getDefaultDisplay().getMode();
            screen = Math.max(mode.getPhysicalWidth(), mode.getPhysicalHeight()) + "," + Math.min(mode.getPhysicalWidth(), mode.getPhysicalHeight());
        } catch (Throwable e) {
            Log.w("Remux", "no size of the screen: " + e);
        }
        return screen + "," + decoder("video/avc") + "," + decoder("video/hevc") + "," + decoder("video/x-vnd.on2.vp9") + "," + decoder("video/av01");
    }

    private static String decoder(String mime) {
        int width = 0, height = 0;
        int[][] sizes = {{7680, 4320}, {3840, 2160}, {2560, 1440}, {1920, 1080}, {1280, 720}};
        try {
            for (MediaCodecInfo info : new MediaCodecList(MediaCodecList.REGULAR_CODECS).getCodecInfos()) {
                if (info.isEncoder()) continue;
                boolean takes = false;
                for (String type : info.getSupportedTypes()) takes |= type.equalsIgnoreCase(mime);
                if (!takes) continue;
                if (Build.VERSION.SDK_INT >= 29 && info.isSoftwareOnly()) continue;
                MediaCodecInfo.VideoCapabilities video = info.getCapabilitiesForType(mime).getVideoCapabilities();
                if (video == null) continue;
                for (int[] size : sizes) {
                    if (video.isSizeSupported(size[0], size[1])) {
                        if (size[0] > width) { width = size[0]; height = size[1]; }
                        break;
                    }
                }
            }
        } catch (Throwable e) {
            Log.w("Remux", "no size of the decoders for " + mime + ": " + e);
        }
        return width + "," + height;
    }
}
