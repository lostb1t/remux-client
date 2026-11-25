package com.github.remux.nsh;

import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Color;
import android.widget.ProgressBar;

/**
 * The system's spinner (Material's turning arc), white, for the app's native component
 * "remux.spinner" (src/ui/spinner_android.rs). Why Java: it is Android's own view, which NSH has
 * no element for.
 */
public final class Spinner {
    private Spinner() {}

    public static ProgressBar make(Context context) {
        ProgressBar bar = new ProgressBar(context);
        bar.setIndeterminate(true);
        bar.setIndeterminateTintList(ColorStateList.valueOf(Color.WHITE));
        bar.setImportantForAccessibility(android.view.View.IMPORTANT_FOR_ACCESSIBILITY_YES);
        bar.setContentDescription("Loading");
        return bar;
    }
}
