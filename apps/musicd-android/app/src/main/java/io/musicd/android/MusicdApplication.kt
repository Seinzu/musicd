package io.musicd.android

import android.app.Application
import coil.ImageLoader
import coil.ImageLoaderFactory
import io.musicd.android.data.MusicdHttp

class MusicdApplication : Application(), ImageLoaderFactory {
    /** Artwork from a server that requires auth needs the pairing token too. */
    override fun newImageLoader(): ImageLoader =
        ImageLoader.Builder(this)
            .okHttpClient { MusicdHttp.client(this) }
            .build()
}
