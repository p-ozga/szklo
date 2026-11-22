package pl.szklo.ui

import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import pl.szklo.ui.theme.SzkloColors

/**
 * „Odwrócenie okna”: cały ekran obraca się jak karta wokół osi Y (0 → 180°, 560 ms).
 * Przednia strona znika po przekroczeniu 90°, tylna pojawia się obrócona o −90° → 0°.
 *
 * Kąt czytany jest tylko w `graphicsLayer {}` (faza rysowania) – animacja nie rekomponuje treści.
 * Jedyna rekompozycja to zamiana stron przy 90° (derivedStateOf); ukryta strona nie jest w drzewie,
 * więc nie przyjmuje dotyku i nie trafia do TalkBack.
 */
@Composable
fun FlipCard(
    flipped: Boolean,
    front: @Composable () -> Unit,
    back: @Composable () -> Unit,
    modifier: Modifier = Modifier,
) {
    val rotation by animateFloatAsState(
        targetValue = if (flipped) 180f else 0f,
        animationSpec = tween(durationMillis = FLIP_MS, easing = FastOutSlowInEasing),
        label = "obrot",
    )
    val showBack by remember { derivedStateOf { rotation > 90f } }

    // Tło pod kartą – widoczne przy krawędziach w trakcie obrotu.
    Box(modifier.fillMaxSize().background(SzkloColors.Background)) {
        if (!showBack) {
            Box(
                Modifier
                    .fillMaxSize()
                    .graphicsLayer {
                        rotationY = rotation
                        cameraDistance = CAMERA_DISTANCE * density
                    },
            ) { front() }
        } else {
            Box(
                Modifier
                    .fillMaxSize()
                    .graphicsLayer {
                        rotationY = rotation - 180f
                        cameraDistance = CAMERA_DISTANCE * density
                    },
            ) { back() }
        }
    }
}

private const val FLIP_MS = 560
/** Odległość kamery (w jednostkach dp) – większa niż domyślna 8, bo obracamy cały ekran. */
private const val CAMERA_DISTANCE = 16f
