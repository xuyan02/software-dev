alter table issues drop constraint issues_status_check;

alter table issues
    add constraint issues_status_check check (
        status = any (array[
            'open',
            'pooled',
            'in_progress',
            'resolved',
            'closed',
            'rejected',
            'duplicate'
        ])
    );

alter table issues add column pool text;

alter table issues
    add constraint issues_pool_check check (
        (status = 'pooled' and pool is not null)
        or (status <> 'pooled' and pool is null)
    );
