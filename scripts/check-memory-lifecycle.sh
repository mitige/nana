#!/usr/bin/env sh
# runs each memory lifecycle function's test on its own, so a broken rule
# names itself instead of hiding in one big run. exits 1 on the first failure.
set -u
cd "$(dirname "$0")/.." || exit 1
fail=0
for t in \
  memory::tests::a_page_round_trips_as_markdown \
  memory::tests::recall_brings_the_body_of_the_pages_the_request_touches \
  memory::tests::recall_marks_a_stale_or_unsure_page_so_it_is_checked_before_use \
  memory::tests::recall_stays_within_its_character_budget \
  memory::tests::recall_counts_each_page_it_hands_over \
  memory::tests::a_page_read_again_counts_its_use_and_restarts_its_age \
  memory::tests::an_unused_page_fades_and_an_often_used_one_fades_slower \
  memory::tests::the_audit_lists_faded_pages_but_never_a_user_page \
  memory::tests::the_audit_finds_pages_that_say_the_same_thing_and_stale_ones \
  memory::tests::tidy_removes_stale_pages_and_copies_but_keeps_user_pages \
  memory::tests::tidy_merges_pages_that_say_nearly_the_same_thing \
  memory::tests::tidy_keeps_a_page_that_is_still_used_even_if_written_long_ago \
  memory::tests::every_write_is_kept_in_the_history_with_its_day \
  memory::tests::consolidation_is_due_on_overlap_and_not_again_on_the_same_memory \
  memory::tests::a_memory_without_overlap_is_not_consolidated \
  memory::tests::a_use_alone_does_not_rearm_consolidation \
  memory::tests::the_model_answer_is_read_into_summaries \
  memory::tests::consolidation_keeps_the_summary_and_the_history_of_what_it_replaces \
  memory::tests::consolidate_asks_the_model_then_applies_its_plan \
  ; do
  if cargo test -q --lib "$t" -- --exact >/dev/null 2>&1; then
    echo "ok    $t"
  else
    echo "FAIL  $t"
    fail=1
  fi
done
exit $fail
