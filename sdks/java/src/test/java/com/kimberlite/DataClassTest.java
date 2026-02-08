package com.kimberlite;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.*;

/**
 * Tests for the {@link DataClass} enum.
 */
class DataClassTest {

    @Test
    void allValuesRoundtrip() {
        for (DataClass dc : DataClass.values()) {
            assertEquals(dc, DataClass.fromValue(dc.getValue()));
        }
    }

    @Test
    void invalidValueThrows() {
        assertThrows(IllegalArgumentException.class, () -> DataClass.fromValue(99));
    }

    @Test
    void negativeValueThrows() {
        assertThrows(IllegalArgumentException.class, () -> DataClass.fromValue(-1));
    }

    @Test
    void ordinalValues() {
        assertEquals(0, DataClass.PUBLIC.getValue());
        assertEquals(1, DataClass.INTERNAL.getValue());
        assertEquals(2, DataClass.CONFIDENTIAL.getValue());
        assertEquals(3, DataClass.RESTRICTED.getValue());
    }

    @Test
    void fourValuesExist() {
        assertEquals(4, DataClass.values().length);
    }

    @Test
    void valueOfByName() {
        assertEquals(DataClass.PUBLIC, DataClass.valueOf("PUBLIC"));
        assertEquals(DataClass.INTERNAL, DataClass.valueOf("INTERNAL"));
        assertEquals(DataClass.CONFIDENTIAL, DataClass.valueOf("CONFIDENTIAL"));
        assertEquals(DataClass.RESTRICTED, DataClass.valueOf("RESTRICTED"));
    }

    @Test
    void valueOfInvalidNameThrows() {
        assertThrows(IllegalArgumentException.class, () -> DataClass.valueOf("UNKNOWN"));
    }
}
