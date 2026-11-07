package app.bridge.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val Background = Color(0xFF0A0B0F)
private val Panel = Color(0xFF15191C)
private val Accent = Color(0xFFD9FF62)
private val Muted = Color(0xFF9298A4)

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent { BridgeHome() }
    }
}

@Composable
private fun BridgeHome() {
    MaterialTheme {
        Surface(Modifier.fillMaxSize(), color = Background) {
            Column(Modifier.padding(24.dp)) {
                Text("BRIDGE", color = Accent, fontSize = 13.sp, fontWeight = FontWeight.Bold, letterSpacing = 3.sp)
                Spacer(Modifier.height(44.dp))
                Text("YOUR ECOSYSTEM", color = Muted, fontSize = 10.sp, letterSpacing = 2.sp)
                Spacer(Modifier.height(10.dp))
                Text("One ecosystem.\nEvery device.", color = Color.White, fontSize = 34.sp, lineHeight = 39.sp, fontWeight = FontWeight.Bold)
                Spacer(Modifier.height(12.dp))
                Text("Bring your phone and computer together in one private workspace.", color = Muted, fontSize = 14.sp, lineHeight = 21.sp)
                Spacer(Modifier.height(28.dp))
                Surface(color = Panel, shape = RoundedCornerShape(18.dp), modifier = Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(20.dp)) {
                        Text("BRIDGE NETWORK", color = Accent, fontSize = 10.sp, letterSpacing = 2.sp)
                        Spacer(Modifier.height(12.dp))
                        Text("No devices paired", color = Color.White, fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
                        Spacer(Modifier.height(6.dp))
                        Text("Secure pairing is coming in a future update.", color = Muted, fontSize = 12.sp)
                        Spacer(Modifier.height(18.dp))
                        Text("PAIRING NOT AVAILABLE YET", color = Accent, fontSize = 9.sp, letterSpacing = 1.sp)
                    }
                }
                Spacer(Modifier.height(26.dp))
                Text("CAPABILITIES", color = Muted, fontSize = 10.sp, letterSpacing = 2.sp)
                Spacer(Modifier.height(12.dp))
                CapabilityRow("Clipboard sharing")
                CapabilityRow("Private file transfer")
                CapabilityRow("Connected devices")
                Spacer(Modifier.weight(1f))
                Text("EARLY FOUNDATION BUILD · LOCAL-FIRST BY DESIGN", color = Muted, fontSize = 9.sp, letterSpacing = 1.sp)
            }
        }
    }
}

@Composable
private fun CapabilityRow(title: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 11.dp), horizontalArrangement = Arrangement.SpaceBetween) {
        Text(title, color = Color.White, fontSize = 13.sp)
        Text("PLANNED", color = Accent, fontSize = 10.sp)
    }
}
