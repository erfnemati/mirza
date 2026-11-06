package dev.erfnemati.mirza

import android.os.Build
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp

private val Brand = Color(0xFFC62828)

/** Material You colors where the phone has them (Android 12+), Mirza's red otherwise; dark with the system. */
@Composable
fun MirzaTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val ctx = LocalContext.current
    val scheme = when {
        Build.VERSION.SDK_INT >= 31 -> if (dark) dynamicDarkColorScheme(ctx) else dynamicLightColorScheme(ctx)
        dark -> darkColorScheme(primary = Color(0xFFFFB4A9), secondary = Color(0xFFE7BDB6))
        else -> lightColorScheme(primary = Brand, secondary = Color(0xFF775651))
    }
    MaterialTheme(colorScheme = scheme, content = content)
}

/** A titled group of settings on a card. */
@Composable
fun Section(title: String?, content: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        if (title != null) {
            Text(
                title,
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.padding(start = 16.dp, top = 8.dp),
            )
        }
        Card(
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Column(Modifier.padding(vertical = 4.dp)) { content() }
        }
    }
}

val clearItem @Composable get() = ListItemDefaults.colors(containerColor = Color.Transparent)

@Composable
fun SwitchRow(title: String, detail: String?, checked: Boolean, onChange: (Boolean) -> Unit) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = detail?.let { { Text(it) } },
        trailingContent = { Switch(checked = checked, onCheckedChange = onChange) },
        colors = clearItem,
        modifier = Modifier.clickable { onChange(!checked) },
    )
}

/**
 * A text setting, saved with the check button or the keyboard's Done. `save` returns why the value
 * was refused, or null; then the field shows the stored value again.
 */
@Composable
fun TextSetting(
    label: String,
    value: String,
    hint: String?,
    singleLine: Boolean = true,
    keyboard: KeyboardType = KeyboardType.Text,
    save: (String) -> String?,
) {
    var saves by remember { mutableIntStateOf(0) }
    var text by remember(value, saves) { mutableStateOf(value) }
    var error by remember(value) { mutableStateOf<String?>(null) }
    val changed = text != value
    val commit = {
        error = save(text)
        if (error == null) saves++
    }
    OutlinedTextField(
        value = text,
        onValueChange = {
            text = it
            error = null
        },
        label = { Text(label) },
        singleLine = singleLine,
        minLines = if (singleLine) 1 else 3,
        isError = error != null,
        supportingText = (error ?: hint)?.let { { Text(it) } },
        trailingIcon = if (changed) {
            { IconButton(onClick = commit) { Icon(Icons.Filled.Check, contentDescription = "Save") } }
        } else {
            null
        },
        keyboardOptions = KeyboardOptions(
            keyboardType = keyboard,
            autoCorrectEnabled = false,
            imeAction = if (singleLine) ImeAction.Done else ImeAction.Default,
        ),
        keyboardActions = KeyboardActions(onDone = { if (changed) commit() }),
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
    )
}

@Composable
fun ButtonRow(content: @Composable RowScope.() -> Unit) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
    ) { content() }
}
