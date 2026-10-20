CREATE TRIGGER t08_r06_abort_delete BEFORE DELETE ON usage_requests BEGIN SELECT RAISE(ABORT, 't08 synthetic delete abort'); END;
