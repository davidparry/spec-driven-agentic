package com.davidparry.workshop.smoke;

import java.io.File;
import java.nio.file.Files;
import java.nio.file.Path;

/**
 * Resolves the installed {@code spec} harness the same way a shell does: the
 * first executable named {@code spec} on {@code PATH}. The smoke jar does
 * not look under {@code harness/target}.
 */
public final class SpecBinary {

    private SpecBinary() {
    }

    /** The {@code spec} on {@code PATH}, or {@code null} if it is not installed. */
    public static Path onPath() {
        return find("spec", System.getenv("PATH"));
    }

    static Path find(String name, String pathEnv) {
        if (pathEnv == null || pathEnv.isBlank()) {
            return null;
        }
        for (String dir : pathEnv.split(File.pathSeparator, -1)) {
            if (dir.isBlank()) {
                continue;
            }
            Path candidate = Path.of(dir).resolve(name);
            if (Files.isRegularFile(candidate) && Files.isExecutable(candidate)) {
                return candidate.toAbsolutePath().normalize();
            }
        }
        return null;
    }
}
