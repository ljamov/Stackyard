# Scheduled tasks

Run commands such as `php artisan schedule:run` on a schedule, with each project's runtime versions.

The **Task scheduler** module (off by default; switch it on in Settings or with
`stackyard modules on scheduler`) runs commands on a schedule, such as Laravel's
`php artisan schedule:run` every minute or a nightly backup script. Each task runs with the
system shell in its project folder, with Stackyard's terminal commands first on `PATH`, so `php`,
`node` and `composer` are the versions that project uses.

Schedules use crontab's five fields, `minute hour day month weekday`: `*/5 * * * *` is every five
minutes, `30 2 * * *` every day at 02:30, `0 9 * * mon-fri` weekdays at 09:00. `@hourly`,
`@daily`, `@weekly`, `@monthly` and `@yearly` work too. Times are this computer's local time.

Tasks run while Stackyard runs: the app (also when only its tray icon is left) or
`stackyard start` in the background. A task still running when it is due again is not started
twice, and a minute missed while the computer slept is not caught up. Each task's output goes to
`logs/task-<id>.log`; the Scheduler screen shows it, and how the last run went.

---

[← Back to the README](../README.md)
