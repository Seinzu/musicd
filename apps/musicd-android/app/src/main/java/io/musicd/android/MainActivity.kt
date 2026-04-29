package io.musicd.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import io.musicd.android.ui.MusicdApp
import io.musicd.android.ui.MusicdTheme
import io.musicd.android.ui.MusicdViewModel

class MainActivity : ComponentActivity() {
    private val viewModel by viewModels<MusicdViewModel>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            MusicdTheme {
                MusicdApp(viewModel = viewModel)
            }
        }
    }
}
