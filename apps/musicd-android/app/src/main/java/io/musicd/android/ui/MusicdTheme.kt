package io.musicd.android.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

private val MusicdColors = darkColorScheme(
    primary = Color(0xFFE8B967),
    onPrimary = Color(0xFF201A12),
    secondary = Color(0xFF8BD3C7),
    background = Color(0xFF0F1217),
    onBackground = Color(0xFFF6F2EA),
    surface = Color(0xFF171C24),
    onSurface = Color(0xFFF6F2EA),
    surfaceVariant = Color(0xFF232A34),
    onSurfaceVariant = Color(0xFFB8C0CC),
    outline = Color(0xFF3A4451),
)

@Composable
fun MusicdTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = if (isSystemInDarkTheme()) MusicdColors else MusicdColors,
        typography = Typography(),
        content = content,
    )
}
