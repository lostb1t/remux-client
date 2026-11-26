package com.github.remux.nsh;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.RectF;
import android.view.View;

import androidx.media3.common.text.Cue;

import java.util.ArrayList;
import java.util.List;

/**
 * Subtitles that are pictures (PGS, DVB, VobSub): each cue's bitmap drawn where the cue says, over
 * the picture of the video (this view is laid out over the surface). The text subtitles are the
 * app's to draw (the `cues` of the snapshot); these have no text. Remux's mpv drew them itself.
 */
final class PictureCues extends View {
    private List<Cue> cues = new ArrayList<>();
    private final Paint paint = new Paint(Paint.FILTER_BITMAP_FLAG | Paint.ANTI_ALIAS_FLAG);
    private final RectF place = new RectF();

    PictureCues(Context context) {
        super(context);
        setWillNotDraw(false);
    }

    /** The cues of now; only the ones with a picture are kept. */
    void show(List<Cue> all) {
        List<Cue> pictures = new ArrayList<>();
        for (Cue cue : all) if (cue.bitmap != null) pictures.add(cue);
        if (pictures.isEmpty() && cues.isEmpty()) return;
        cues = pictures;
        invalidate();
    }

    @Override
    protected void onDraw(Canvas canvas) {
        int width = getWidth(), height = getHeight();
        if (width == 0 || height == 0) return;
        for (Cue cue : cues) {
            Bitmap bitmap = cue.bitmap;
            if (bitmap == null) continue;
            // The size as parts of the picture: the width the cue gives, the height its own or by
            // the bitmap's proportions.
            float w = (cue.size == Cue.DIMEN_UNSET ? 0.5f : cue.size) * width;
            float h = cue.bitmapHeight == Cue.DIMEN_UNSET
                    ? w * bitmap.getHeight() / bitmap.getWidth()
                    : cue.bitmapHeight * height;
            float x = (cue.position == Cue.DIMEN_UNSET ? 0.5f : cue.position) * width;
            float y = (cue.line == Cue.DIMEN_UNSET ? 0.9f : cue.line) * height;
            // Where the point is on the bitmap: its start, middle or end.
            if (cue.positionAnchor == Cue.ANCHOR_TYPE_END) x -= w;
            else if (cue.positionAnchor == Cue.ANCHOR_TYPE_MIDDLE) x -= w / 2;
            if (cue.lineAnchor == Cue.ANCHOR_TYPE_END) y -= h;
            else if (cue.lineAnchor == Cue.ANCHOR_TYPE_MIDDLE) y -= h / 2;
            place.set(x, y, x + w, y + h);
            canvas.drawBitmap(bitmap, null, place, paint);
        }
    }
}
