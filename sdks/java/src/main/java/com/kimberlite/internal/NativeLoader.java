package com.kimberlite.internal;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;

/**
 * Handles loading the native Kimberlite FFI library.
 *
 * <p>Loading strategy (in order):
 * <ol>
 *   <li>Attempt {@code System.loadLibrary("kimberlite_ffi")} using
 *       {@code java.library.path}</li>
 *   <li>Extract the platform-specific library from the JAR's
 *       {@code /native/<os>-<arch>/} resources into a temporary directory
 *       and load from there</li>
 * </ol>
 *
 * <p>Supported platforms:
 * <ul>
 *   <li>Linux x86_64, aarch64</li>
 *   <li>macOS x86_64, aarch64</li>
 *   <li>Windows x86_64</li>
 * </ul>
 */
public final class NativeLoader {

    private static volatile boolean loaded = false;

    private NativeLoader() {
        // Utility class; prevent instantiation
    }

    /**
     * Loads the native library. This method is idempotent and thread-safe.
     *
     * @throws UnsatisfiedLinkError if the library cannot be found on
     *         {@code java.library.path} or extracted from JAR resources
     */
    public static void load() {
        if (loaded) {
            return;
        }

        synchronized (NativeLoader.class) {
            if (loaded) {
                return;
            }

            // Strategy 1: Try java.library.path
            try {
                System.loadLibrary("kimberlite_ffi");
                loaded = true;
                return;
            } catch (UnsatisfiedLinkError e) {
                // Fall through to resource extraction
            }

            // Strategy 2: Extract from JAR resources
            String os = detectOs();
            String arch = detectArch();
            String libName = libraryName(os);
            String resourcePath = "/native/" + os + "-" + arch + "/" + libName;

            try (InputStream in = NativeLoader.class.getResourceAsStream(resourcePath)) {
                if (in == null) {
                    throw new UnsatisfiedLinkError(
                        "Native library not found: " + resourcePath
                        + " (os=" + os + ", arch=" + arch + ")"
                        + ". Ensure the library is on java.library.path"
                        + " or bundled in the JAR."
                    );
                }

                Path tempDir = Files.createTempDirectory("kimberlite-native");
                Path tempLib = tempDir.resolve(libName);
                Files.copy(in, tempLib, StandardCopyOption.REPLACE_EXISTING);

                // Mark for cleanup on JVM exit
                tempLib.toFile().deleteOnExit();
                tempDir.toFile().deleteOnExit();

                System.load(tempLib.toAbsolutePath().toString());
                loaded = true;
            } catch (IOException e) {
                throw new UnsatisfiedLinkError(
                    "Failed to extract native library: " + e.getMessage()
                );
            }
        }
    }

    /**
     * Detects the current operating system.
     *
     * @return one of "linux", "macos", or "windows"
     * @throws UnsatisfiedLinkError if the OS is not supported
     */
    static String detectOs() {
        String osName = System.getProperty("os.name", "").toLowerCase();

        if (osName.contains("linux")) {
            return "linux";
        }
        if (osName.contains("mac") || osName.contains("darwin")) {
            return "macos";
        }
        if (osName.contains("windows")) {
            return "windows";
        }

        throw new UnsatisfiedLinkError("Unsupported operating system: " + osName);
    }

    /**
     * Detects the current CPU architecture.
     *
     * @return one of "x86_64" or "aarch64"
     * @throws UnsatisfiedLinkError if the architecture is not supported
     */
    static String detectArch() {
        String archName = System.getProperty("os.arch", "").toLowerCase();

        if (archName.equals("amd64") || archName.equals("x86_64")) {
            return "x86_64";
        }
        if (archName.equals("aarch64") || archName.equals("arm64")) {
            return "aarch64";
        }

        throw new UnsatisfiedLinkError("Unsupported architecture: " + archName);
    }

    /**
     * Returns the platform-specific library filename.
     *
     * @param os the operating system identifier
     * @return the library filename (e.g., "libkimberlite_ffi.so")
     */
    static String libraryName(String os) {
        return switch (os) {
            case "linux" -> "libkimberlite_ffi.so";
            case "macos" -> "libkimberlite_ffi.dylib";
            case "windows" -> "kimberlite_ffi.dll";
            default -> throw new UnsatisfiedLinkError("Unsupported OS: " + os);
        };
    }
}
