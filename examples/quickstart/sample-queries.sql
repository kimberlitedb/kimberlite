-- Kimberlite Sample Queries
-- Run these in the REPL or with `kimberlite query`

-- Create tables
CREATE TABLE patients (
  id BIGINT NOT NULL,
  name TEXT,
  date_of_birth TEXT,
  email TEXT
);

CREATE TABLE appointments (
  id BIGINT NOT NULL,
  patient_id BIGINT NOT NULL,
  scheduled_at TEXT,
  status TEXT
);

-- Insert patient data
INSERT INTO patients (id, name, date_of_birth, email) VALUES
  (1, 'Jane Doe', '1990-05-15', 'jane@example.com');

INSERT INTO patients (id, name, date_of_birth, email) VALUES
  (2, 'John Smith', '1985-03-22', 'john@example.com');

INSERT INTO patients (id, name, date_of_birth, email) VALUES
  (3, 'Alice Johnson', '1978-11-08', 'alice@example.com');

-- Insert appointment data
INSERT INTO appointments (id, patient_id, scheduled_at, status) VALUES
  (1, 1, '2024-01-15 09:00:00', 'completed');

INSERT INTO appointments (id, patient_id, scheduled_at, status) VALUES
  (2, 1, '2024-02-20 14:30:00', 'scheduled');

INSERT INTO appointments (id, patient_id, scheduled_at, status) VALUES
  (3, 2, '2024-01-20 10:00:00', 'cancelled');

-- Query examples
-- List all patients
SELECT * FROM patients;

-- Find specific patient
SELECT * FROM patients WHERE id = 1;

-- List scheduled appointments
SELECT * FROM appointments WHERE status = 'scheduled';

-- Count patients (note: aggregates not yet supported)
-- SELECT COUNT(*) FROM patients;
