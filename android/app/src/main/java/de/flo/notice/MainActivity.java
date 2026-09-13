package de.flo.notice;

import android.app.Activity;
import android.graphics.Color;
import android.os.Build;
import android.os.Bundle;
import android.util.Log;
import android.view.View;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowManager;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.FrameLayout;

import java.io.File;
import java.io.IOException;
import java.net.InetSocketAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.util.ArrayList;
import java.util.List;

/**
 * Hostet die Notice-Web-App in einer Vollbild-WebView.
 *
 * Beim Start wird der mitgelieferte `notice`-Server (aarch64-Linux-Binary in
 * jniLibs, dort als libnotice.so verpackt) mit einem freien Loopback-Port
 * gestartet. Arbeitsverzeichnis sind die app-eigenen Dateien (filesDir):
 * config.toml, notice.db, contacts.db und backups liegen dort und
 * überleben Neuinstallationen der Bibliothek.
 *
 * Edge-to-Edge-Rendering: Status- und Navigationsleiste sind transparent,
 * die WebView liegt in einem FrameLayout, das per WindowInsets um den
 * Status-/Navigationsbalken und den Display-Cutout gepaddet wird. Dadurch
 * wird der Inhalt (Header/Buttons) unter der Uhrzeit/Kamera-Linse nach
 * unten verschoben und nie verdeckt.
 */
public class MainActivity extends Activity {
    private static final String TAG = "Notice";
    private static final String BIN = "libnotice.so";

    private Process server;
    private WebView webView;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        WebView.setWebContentsDebuggingEnabled(true);

        /* --- Edge-to-Edge: transparente Systemleisten --- */
        final Window w = getWindow();
        w.setStatusBarColor(Color.TRANSPARENT);
        w.setNavigationBarColor(Color.TRANSPARENT);

        /* Display-Cutout im Kurz-Modus (API 28+) */
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            WindowManager.LayoutParams lp = w.getAttributes();
            lp.layoutInDisplayCutoutMode =
                    WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
            w.setAttributes(lp);
        }

        /* Content unter Systemleisten zeichnen (API 30+: modern; älter: legacy flags) */
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            w.setDecorFitsSystemWindows(false);
        } else {
            w.getDecorView().setSystemUiVisibility(
                    View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                            | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                            | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION);
        }

        /* --- WebView --- */
        /* Die WebView zeichnet ihren Inhalt trotz View-Padding immer bis an die
           Ränder (100vh bezieht sich auf die volle WebView-Größe). Damit nichts
           hinter Uhrzeit/Kamera-Loch landet, wird sie in ein FrameLayout gepackt,
           das per WindowInsets gepaddet wird - so schrumpft der für die WebView
           verfügbare Bereich wirklich und die App rückt nach unten. */
        FrameLayout root = new FrameLayout(this);
        root.setBackgroundColor(Color.parseColor("#18181b"));
        setContentView(root);

        webView = new WebView(this);
        WebSettings s = webView.getSettings();
        s.setJavaScriptEnabled(true);
        s.setDomStorageEnabled(true);
        webView.setWebViewClient(new WebViewClient());
        root.addView(webView,
                new FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT,
                        FrameLayout.LayoutParams.MATCH_PARENT));

        /* Container um Status-/Nav-Balken + Display-Cutout padden */
        root.setOnApplyWindowInsetsListener(new View.OnApplyWindowInsetsListener() {
            @Override
            public WindowInsets onApplyWindowInsets(View v, WindowInsets insets) {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                    android.graphics.Insets barInsets =
                            insets.getInsets(WindowInsets.Type.systemBars()
                                    | WindowInsets.Type.displayCutout());
                    v.setPadding(barInsets.left, barInsets.top,
                            barInsets.right, barInsets.bottom);
                } else {
                    v.setPadding(
                            insets.getSystemWindowInsetLeft(),
                            insets.getSystemWindowInsetTop(),
                            insets.getSystemWindowInsetRight(),
                            insets.getSystemWindowInsetBottom());
                }
                return insets;
            }
        });

        startServer();
    }

    private void startServer() {
        final int port = findFreePort();
        final File bin = new File(getApplicationInfo().nativeLibraryDir, BIN);
        if (!bin.canExecute()) {
            Log.e(TAG, "Binärdatei nicht ausführbar: " + bin);
            return;
        }

        List<String> cmd = new ArrayList<>();
        cmd.add(bin.getAbsolutePath());
        cmd.add("--port");
        cmd.add(String.valueOf(port));

        try {
            ProcessBuilder pb = new ProcessBuilder(cmd);
            pb.directory(getFilesDir());
            pb.redirectErrorStream(true);
            pb.redirectOutput(new File(getFilesDir(), "notice.log"));
            server = pb.start();
        } catch (IOException e) {
            Log.e(TAG, "Serverstart fehlgeschlagen", e);
            return;
        }

        new Thread(new Poller(port)).start();
    }

    private int findFreePort() {
        try (ServerSocket socket = new ServerSocket(0)) {
            return socket.getLocalPort();
        } catch (IOException e) {
            return 8080;
        }
    }

    private boolean canConnect(int port) {
        try (Socket socket = new Socket()) {
            socket.connect(new InetSocketAddress("127.0.0.1", port), 300);
            return true;
        } catch (IOException e) {
            return false;
        }
    }

    /** Wartet, bis der Server antwortet, und lädt dann die App. */
    private final class Poller implements Runnable {
        private final int port;

        Poller(int port) {
            this.port = port;
        }

        @Override
        public void run() {
            long deadline = System.currentTimeMillis() + 15000;
            while (System.currentTimeMillis() < deadline) {
                if (server == null || !server.isAlive()) {
                    Log.e(TAG, "Serverprozess beendet");
                    return;
                }
                if (canConnect(port)) {
                    final String url = "http://127.0.0.1:" + port + "/";
                    runOnUiThread(() -> webView.loadUrl(url));
                    return;
                }
                try {
                    Thread.sleep(200);
                } catch (InterruptedException e) {
                    return;
                }
            }
            Log.e(TAG, "Timeout: Server nicht erreichbar");
        }
    }

    @Override
    protected void onDestroy() {
        if (server != null) {
            server.destroy();
            server = null;
        }
        if (webView != null) {
            webView.destroy();
            webView = null;
        }
        super.onDestroy();
    }

    @Override
    public void onBackPressed() {
        if (webView != null && webView.canGoBack()) {
            webView.goBack();
        } else {
            super.onBackPressed();
        }
    }
}
