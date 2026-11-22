package pl.szklo.ui

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import pl.szklo.ui.backside.LocalPlayerActions
import pl.szklo.ui.backside.PlaceholderPlayerControls
import pl.szklo.ui.backside.PlayerBackside
import pl.szklo.ui.backside.PlayerControlsBackside
import pl.szklo.ui.components.PanelState

/**
 * Korzeń UI: karta z dwiema stronami – przód (pokrętło + panel modułów) i tył (odtwarzacz).
 * Stan panelu dolnego jest trzymany tutaj, żeby przetrwał obrót karty.
 */
@Composable
fun SzkloRoot(
    vm: SzkloViewModel,
    playerControls: PlayerControlsBackside = PlaceholderPlayerControls,
) {
    val context = LocalContext.current
    var flipped by rememberSaveable { mutableStateOf(false) }
    val panelState = remember { PanelState() }

    BackHandler(enabled = flipped) { flipped = false }

    // Android 13+: powiadomienie usługi pierwszoplanowej. Prosimy przy pierwszej akcji, która go wymaga;
    // odmowa nie blokuje działania (usługa działa, tylko bez widocznego powiadomienia).
    var pendingAction by remember { mutableStateOf<(() -> Unit)?>(null) }
    val notifPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        pendingAction?.invoke()
        pendingAction = null
    }
    val withNotifications: (() -> Unit) -> Unit = remember(context, notifPermission) {
        { action ->
            val needs = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
                ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
            if (needs) {
                pendingAction = action
                notifPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
            } else {
                action()
            }
        }
    }

    val pickAudio = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) withNotifications { vm.playLocal(uri) }
    }

    FlipCard(
        flipped = flipped,
        front = {
            FrontScreen(
                vm = vm,
                panelState = panelState,
                onFlip = { flipped = true },
                onGlobalEffects = { on -> if (on) withNotifications { vm.setGlobalEffects(true) } else vm.setGlobalEffects(false) },
            )
        },
        back = {
            // Stan odtwarzacza (postęp co 250 ms) czytany tylko na stronie tylnej.
            val player by vm.player.collectAsStateWithLifecycle()
            PlayerBackside(
                controls = playerControls,
                player = player,
                coreAvailable = vm.coreAvailable,
                local = LocalPlayerActions(
                    pickFile = { pickAudio.launch(arrayOf("audio/*")) },
                    pause = vm::pauseLocal,
                    resume = vm::resumeLocal,
                    stop = vm::stopLocal,
                ),
                onApplyPreset = vm::applyPreset,
                onBack = { flipped = false },
            )
        },
    )
}
