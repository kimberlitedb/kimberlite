/**
 * Lightweight SQL query builder — mirrors the Rust `Query` type.
 *
 * Not an ORM. Emits a `(sql, params)` tuple you pass to `client.query()`.
 *
 * @example
 * ```ts
 * import { Query, ValueBuilder } from '@kimberlite/client';
 *
 * const { sql, params } = Query.from('patients')
 *   .select(['id', 'name', 'dob'])
 *   .whereEq('tenant_id', ValueBuilder.bigint(42n))
 *   .whereEq('active', ValueBuilder.boolean(true))
 *   .orderBy('name')
 *   .limit(100)
 *   .build();
 *
 * const result = await client.query(sql, params);
 * ```
 */

import { Value } from './value';

type Cmp = '=' | '<' | '>' | '<=' | '>=' | '!=';

interface Predicate {
  column: string;
  cmp: Cmp;
  value: Value;
}

export class Query {
  private columns: string[] = [];
  private wheres: Predicate[] = [];
  private orderCol: string | null = null;
  private orderDesc = false;
  private limitN: number | null = null;

  private constructor(private readonly fromTable: string) {}

  /** Start a query from `table`. */
  static from(table: string): Query {
    return new Query(table);
  }

  select(columns: string[]): this {
    this.columns = [...columns];
    return this;
  }

  whereEq(column: string, value: Value): this {
    this.wheres.push({ column, cmp: '=', value });
    return this;
  }

  whereLt(column: string, value: Value): this {
    this.wheres.push({ column, cmp: '<', value });
    return this;
  }

  whereGt(column: string, value: Value): this {
    this.wheres.push({ column, cmp: '>', value });
    return this;
  }

  whereLe(column: string, value: Value): this {
    this.wheres.push({ column, cmp: '<=', value });
    return this;
  }

  whereGe(column: string, value: Value): this {
    this.wheres.push({ column, cmp: '>=', value });
    return this;
  }

  whereNe(column: string, value: Value): this {
    this.wheres.push({ column, cmp: '!=', value });
    return this;
  }

  orderBy(column: string): this {
    this.orderCol = column;
    this.orderDesc = false;
    return this;
  }

  orderByDesc(column: string): this {
    this.orderCol = column;
    this.orderDesc = true;
    return this;
  }

  limit(n: number): this {
    this.limitN = n;
    return this;
  }

  build(): { sql: string; params: Value[] } {
    const cols = this.columns.length === 0 ? '*' : this.columns.join(', ');
    let sql = `SELECT ${cols} FROM ${this.fromTable}`;
    const params: Value[] = [];

    this.wheres.forEach((p, i) => {
      sql += i === 0 ? ' WHERE ' : ' AND ';
      sql += `${p.column} ${p.cmp} $${i + 1}`;
      params.push(p.value);
    });

    if (this.orderCol) {
      sql += ` ORDER BY ${this.orderCol}${this.orderDesc ? ' DESC' : ''}`;
    }
    if (this.limitN !== null) {
      sql += ` LIMIT ${this.limitN}`;
    }
    return { sql, params };
  }
}
