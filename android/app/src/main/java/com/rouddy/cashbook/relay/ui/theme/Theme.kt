package com.rouddy.cashbook.relay.ui.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

// 웹 가계부의 accent 색 (web/src/styles.css)
private val Accent = Color(0xFF2A78D6)
private val AccentDark = Color(0xFF6DA7EC)

@Composable
fun RelayTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val context = LocalContext.current
    val colors = when {
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        dark -> darkColorScheme(primary = AccentDark)
        else -> lightColorScheme(primary = Accent)
    }
    MaterialTheme(colorScheme = colors, content = content)
}
