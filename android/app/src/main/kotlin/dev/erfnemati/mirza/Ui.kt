package dev.erfnemati.mirza

import android.os.Build
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
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
import kotlin.math.roundToInt

/** The logo's teal, light and dark ends of its gradient. */
val LogoLight = Color(0xFF2DD4BF)
val LogoDark = Color(0xFF0F766E)

/** Material You colors where the phone has them (Android 12+), the logo's teal otherwise; dark with the system. */
@Composable
fun MirzaTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val ctx = LocalContext.current
    val scheme = when {
        Build.VERSION.SDK_INT >= 31 -> if (dark) dynamicDarkColorScheme(ctx) else dynamicLightColorScheme(ctx)
        dark -> darkColorScheme(primary = Color(0xFF5EEAD4), secondary = Color(0xFFB0CCC6))
        else -> lightColorScheme(primary = LogoDark, secondary = Color(0xFF4A6360))
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

/** A whole number typed in, checked against `range`. */
@Composable
fun NumberSetting(label: String, value: Int, range: IntRange, hint: String?, save: (Int) -> String?) {
    TextSetting(label, value.toString(), hint, keyboard = KeyboardType.Number) { text ->
        val n = text.trim().toIntOrNull()
        when {
            n == null -> "Enter a whole number."
            n !in range -> "Enter ${range.first} to ${range.last}."
            else -> save(n)
        }
    }
}

/** A value picked on a slider; saved when the finger lets go. */
@Composable
fun SliderSetting(
    label: String,
    value: Int,
    range: IntRange,
    step: Int,
    shown: (Int) -> String,
    save: (Int) -> Unit,
) {
    var v by remember(value) { mutableFloatStateOf(value.toFloat()) }
    Column(Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            Text(shown(v.roundToInt()), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Slider(
            value = v,
            onValueChange = { v = it },
            onValueChangeFinished = { save(v.roundToInt()) },
            valueRange = range.first.toFloat()..range.last.toFloat(),
            steps = (range.last - range.first) / step - 1,
        )
    }
}

/** One of a few choices, in a menu; the row shows the chosen one under its title. */
@Composable
fun <T> ChoiceRow(title: String, detail: String?, value: T, options: List<Pair<T, String>>, pick: (T) -> Unit) {
    var open by remember { mutableStateOf(false) }
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = {
            Column {
                Text(
                    options.firstOrNull { it.first == value }?.second ?: value.toString(),
                    color = MaterialTheme.colorScheme.primary,
                    style = MaterialTheme.typography.bodyLarge,
                )
                if (detail != null) Text(detail)
            }
        },
        trailingContent = {
            Box {
                Icon(Icons.Filled.ArrowDropDown, contentDescription = null)
                DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                    options.forEach { (v, name) ->
                        DropdownMenuItem(text = { Text(name) }, onClick = {
                            open = false
                            if (v != value) pick(v)
                        })
                    }
                }
            }
        },
        colors = clearItem,
        modifier = Modifier.clickable { open = true },
    )
}

/** Choices shown all at once, one line each. */
@Composable
fun <T> RadioRows(value: T, options: List<Triple<T, String, String?>>, pick: (T) -> Unit) {
    options.forEach { (v, name, blurb) ->
        ListItem(
            leadingContent = { RadioButton(selected = v == value, onClick = { pick(v) }) },
            headlineContent = { Text(name) },
            supportingContent = blurb?.let { { Text(it) } },
            colors = clearItem,
            modifier = Modifier.clickable { pick(v) },
        )
    }
}

/** A short explanation above a screen's settings. */
@Composable
fun Intro(text: String) {
    Text(text, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 16.dp))
}

/** A highlighted line that asks for something, e.g. a missing key. */
@Composable
fun Notice(text: String) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.errorContainer),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Text(text, color = MaterialTheme.colorScheme.onErrorContainer, modifier = Modifier.padding(16.dp))
    }
}
