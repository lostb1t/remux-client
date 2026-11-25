package com.github.remux.nsh;

import android.app.Activity;
import android.content.Intent;
import android.os.Bundle;

/**
 * The app's entry in an Android TV launcher. Why Java: the TV launcher lists only activities with
 * the LEANBACK_LAUNCHER category, and the manifest NSH writes gives its own activity just
 * LAUNCHER; NSH lets an app add an activity of its own (nsh.json "android.application"), not
 * change its own. This one only opens the app's activity, and is gone.
 */
public final class TvLauncher extends Activity {
    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        Intent app = getPackageManager().getLaunchIntentForPackage(getPackageName());
        if (app != null) {
            app.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            startActivity(app);
        }
        finish();
    }
}
