package de.flo.notice;

import android.app.Activity;
import android.content.ContentValues;
import android.content.Intent;
import android.graphics.Color;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.provider.MediaStore;
import android.util.Log;
import android.view.View;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowManager;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.FrameLayout;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.OutputStream;
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
 * Edge-to-Edge-Rendering: transparente Systemleisten, FrameLayout-Padding.
 * JavaScript-Interface `NoticeBridge` fuer Datei-Downloads (Export).
 * WebChromeClient fuer Datei-Auswahl (Import).
 */
public class MainActivity extends Activity {
    private static final String TAG = "Notice";
    private static final String BIN = "libnotice.so";
    private static final int FILE_CHOOSER_REQUEST = 1001;

    private Process server;
    private WebView webView;
    private ValueCallback<Uri[]> fileUploadCallback;

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
        FrameLayout root = new FrameLayout(this);
        root.setBackgroundColor(Color.parseColor("#18181b"));
        setContentView(root);

        webView = new WebView(this);
        WebSettings s = webView.getSettings();
        s.setJavaScriptEnabled(true);
        s.setDomStorageEnabled(true);

        /* JS-Interface fuer Datei-Downloads (Export) */
        webView.addJavascriptInterface(new NoticeBridge(), "NoticeBridge");

        /* WebChromeClient fuer Datei-Auswahl (Import) */
        webView.setWebChromeClient(new WebChromeClient() {
            @Override
            public boolean onShowFileChooser(WebView webView,
                                             ValueCallback<Uri[]> filePathCallback,
                                             FileChooserParams fileChooserParams) {
                if (fileUploadCallback != null) {
                    fileUploadCallback.onReceiveValue(null);
                }
                fileUploadCallback = filePathCallback;
                Intent intent = new Intent(Intent.ACTION_GET_CONTENT);
                intent.addCategory(Intent.CATEGORY_OPENABLE);
                intent.setType("*/*");
                String[] mimeTypes = fileChooserParams.getAcceptTypes();
                if (mimeTypes != null && mimeTypes.length == 1 && !mimeTypes[0].isEmpty()) {
                    intent.setType(mimeTypes[0]);
                }
                try {
                    startActivityForResult(Intent.createChooser(intent, "Datei auswählen"), FILE_CHOOSER_REQUEST);
                } catch (Exception e) {
                    Log.e(TAG, "Datei-Auswahl fehlgeschlagen", e);
                    fileUploadCallback.onReceiveValue(null);
                    fileUploadCallback = null;
                }
                return true;
            }
        });

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

    /** Ergebnis der Datei-Auswahl an die WebView zurueckgeben */
    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == FILE_CHOOSER_REQUEST) {
            if (fileUploadCallback != null) {
                Uri[] results = null;
                if (resultCode == RESULT_OK && data != null) {
                    Uri uri = data.getData();
                    if (uri != null) {
                        results = new Uri[]{uri};
                    }
                }
                fileUploadCallback.onReceiveValue(results);
                fileUploadCallback = null;
            }
        }
    }

    /**
     * JS-Interface fuer Download/Export.
     * `window.NoticeBridge.saveFile(filename, content, mime)` speichert die
     * Datei ueber MediaStore (API 29+) oder in die App-spezifische Datei.
     */
    private class NoticeBridge {
        @android.webkit.JavascriptInterface
        public boolean saveFile(String filename, String content, String mime) {
            try {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    /* API 29+: MediaStore.Downloads */
                    ContentValues cv = new ContentValues();
                    cv.put(MediaStore.Downloads.DISPLAY_NAME, filename);
                    cv.put(MediaStore.Downloads.MIME_TYPE, mime != null ? mime : "text/plain");
                    cv.put(MediaStore.Downloads.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS);
                    Uri uri = getContentResolver().insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, cv);
                    if (uri != null) {
                        try (OutputStream os = getContentResolver().openOutputStream(uri)) {
                            if (os != null) {
                                os.write(content.getBytes("UTF-8"));
                                os.flush();
                                Log.i(TAG, "Datei gespeichert: " + filename);
                                return true;
                            }
                        }
                    }
                } else {
                    /* API < 29: Externer App-Ordner */
                    File dir = getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS);
                    if (dir != null) {
                        if (!dir.exists()) dir.mkdirs();
                        File file = new File(dir, filename);
                        try (FileOutputStream fos = new FileOutputStream(file)) {
                            fos.write(content.getBytes("UTF-8"));
                            fos.flush();
                            Log.i(TAG, "Datei gespeichert: " + file.getAbsolutePath());
                            return true;
                        }
                    }
                }
            } catch (Exception e) {
                Log.e(TAG, "saveFile fehlgeschlagen: " + filename, e);
            }
            return false;
        }
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

    @SuppressWarnings("deprecation")
    @Override
    public void onBackPressed() {
        if (webView != null && webView.canGoBack()) {
            webView.goBack();
        } else {
            super.onBackPressed();
        }
    }
}
