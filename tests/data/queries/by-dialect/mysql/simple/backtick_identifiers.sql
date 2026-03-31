-- MySQL backtick identifier syntax
-- Tests: Backtick quoting, reserved word as column name

SELECT `user`.`select`, `user`.`from`
FROM `users` AS `user`
WHERE `user`.`id` = 1;
