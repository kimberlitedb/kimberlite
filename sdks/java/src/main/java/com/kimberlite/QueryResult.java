package com.kimberlite;

import java.util.Collections;
import java.util.List;

/**
 * Holds the result of a SQL query execution.
 *
 * <p>A QueryResult consists of an ordered list of column names and
 * a list of rows, where each row is a list of {@link QueryValue} cells
 * corresponding to the columns.
 *
 * <p>Both columns and rows are unmodifiable after construction.
 */
public final class QueryResult {

    private final List<String> columns;
    private final List<List<QueryValue>> rows;

    /**
     * Creates a new QueryResult.
     *
     * @param columns the column names in order
     * @param rows the result rows, each containing values aligned with columns
     */
    public QueryResult(List<String> columns, List<List<QueryValue>> rows) {
        this.columns = columns != null
            ? Collections.unmodifiableList(columns)
            : Collections.emptyList();
        this.rows = rows != null
            ? Collections.unmodifiableList(rows)
            : Collections.emptyList();
    }

    /**
     * Returns the column names.
     *
     * @return an unmodifiable list of column names
     */
    public List<String> getColumns() {
        return columns;
    }

    /**
     * Returns the result rows.
     *
     * @return an unmodifiable list of rows
     */
    public List<List<QueryValue>> getRows() {
        return rows;
    }

    /**
     * Returns the number of rows in the result.
     *
     * @return the row count
     */
    public int getRowCount() {
        return rows.size();
    }

    /**
     * Returns the number of columns in the result.
     *
     * @return the column count
     */
    public int getColumnCount() {
        return columns.size();
    }

    @Override
    public String toString() {
        return "QueryResult{columns=" + columns + ", rowCount=" + rows.size() + "}";
    }
}
