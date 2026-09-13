package de.flo.notice;

import android.app.Activity;
import android.os.Bundle;
import android.util.Log;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;

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

        webView = new WebView(this);
        WebSettings s = webView.getSettings();
        s.setJavaScriptEnabled(true);
        s.setDomStorageEnabled(true);
        webView.setWebViewClient(new WebViewClient());
        setContentView(webView);

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