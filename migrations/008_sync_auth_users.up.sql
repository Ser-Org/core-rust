-- Sync auth.users → public.users when running against Supabase.
-- When the auth schema is absent (plain Postgres / CI), we skip the trigger.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'auth' AND table_name = 'users') THEN
        CREATE OR REPLACE FUNCTION public.handle_new_user()
        RETURNS trigger
        LANGUAGE plpgsql
        SECURITY DEFINER
        SET search_path = public
        AS $fn$
        BEGIN
            INSERT INTO public.users (id, email, created_at, updated_at)
            VALUES (NEW.id, NEW.email, now(), now())
            ON CONFLICT (id) DO NOTHING;
            RETURN NEW;
        END;
        $fn$;

        EXECUTE 'DROP TRIGGER IF EXISTS on_auth_user_created ON auth.users';
        EXECUTE 'CREATE TRIGGER on_auth_user_created
            AFTER INSERT ON auth.users
            FOR EACH ROW
            EXECUTE FUNCTION public.handle_new_user()';

        EXECUTE 'INSERT INTO public.users (id, email, created_at, updated_at)
                 SELECT id, email, created_at, now()
                 FROM auth.users
                 WHERE id NOT IN (SELECT id FROM public.users)
                 ON CONFLICT (id) DO NOTHING';
    END IF;
END $$;
