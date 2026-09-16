package com.davidparry.workshop.smoke;

import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.condition.EnabledOnOs;
import org.junit.jupiter.api.condition.OS;
import org.junit.jupiter.api.io.TempDir;

import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.attribute.PosixFilePermissions;

import static org.assertj.core.api.Assertions.assertThat;

class BddBinaryTest {

    @TempDir
    Path temp;

    @Test
    @DisplayName("a missing PATH yields no binary")
    void missingPath() {
        assertThat(BddBinary.find("bdd", null)).isNull();
        assertThat(BddBinary.find("bdd", "  ")).isNull();
    }

    @Test
    @DisplayName("blank PATH entries are skipped")
    void blankEntriesSkipped() {
        assertThat(BddBinary.find("bdd", File.pathSeparator + File.pathSeparator)).isNull();
    }

    @Test
    @EnabledOnOs({OS.MAC, OS.LINUX})
    @DisplayName("the first executable named bdd on PATH is used")
    void firstExecutableWins() throws IOException {
        Path first = executable("first", "bdd");
        Path second = executable("second", "bdd");
        String path = first.getParent() + File.pathSeparator + second.getParent();
        assertThat(BddBinary.find("bdd", path)).isEqualTo(first.toAbsolutePath().normalize());
    }

    @Test
    @EnabledOnOs({OS.MAC, OS.LINUX})
    @DisplayName("a non-executable file on PATH is ignored")
    void nonExecutableIgnored() throws IOException {
        Path file = temp.resolve("bdd");
        Files.writeString(file, "not a binary");
        Files.setPosixFilePermissions(file, PosixFilePermissions.fromString("rw-r--r--"));
        assertThat(BddBinary.find("bdd", temp.toString())).isNull();
    }

    @Test
    @EnabledOnOs({OS.MAC, OS.LINUX})
    @DisplayName("a directory named bdd on PATH is ignored")
    void directoryIgnored() throws IOException {
        Path dir = temp.resolve("bdd");
        Files.createDirectory(dir);
        assertThat(BddBinary.find("bdd", temp.toString())).isNull();
    }

    @Test
    @DisplayName("onPath is the bdd found on the process PATH")
    void onPathUsesProcessPath() {
        assertThat(BddBinary.onPath()).isEqualTo(BddBinary.find("bdd", System.getenv("PATH")));
    }

    private Path executable(String folder, String name) throws IOException {
        Path dir = Files.createDirectory(temp.resolve(folder));
        Path binary = dir.resolve(name);
        Files.writeString(binary, "#!/bin/sh\n");
        Files.setPosixFilePermissions(binary, PosixFilePermissions.fromString("rwxr-xr-x"));
        return binary;
    }
}
