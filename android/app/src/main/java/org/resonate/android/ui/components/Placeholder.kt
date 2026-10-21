package org.resonate.android.ui.components

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Construction
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

@Composable
fun Placeholder(title: String, back: Boolean = true) {
    Page(title, back = back, actions = { if (!back) SettingsAction() }) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            EmptyState(Icons.Rounded.Construction, "$title is being rebuilt")
        }
    }
}
