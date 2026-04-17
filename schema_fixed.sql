--
-- PostgreSQL database dump
--

-- Dumped from database version 17.5
-- Dumped by pg_dump version 17.5

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: meeting_attendance; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.meeting_attendance (
    id bigint NOT NULL,
    room_id text NOT NULL,
    user_id text NOT NULL,
    first_joined_at timestamp without time zone,
    last_left_at timestamp without time zone,
    total_active_seconds bigint DEFAULT 0,
    reconnect_count integer DEFAULT 0,
    status text DEFAULT 'left'::text
);


ALTER TABLE public.meeting_attendance OWNER TO postgres;

--
-- Name: meeting_attendance_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.meeting_attendance_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.meeting_attendance_id_seq OWNER TO postgres;

--
-- Name: meeting_attendance_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.meeting_attendance_id_seq OWNED BY public.meeting_attendance.id;


--
-- Name: participant_sessions; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.participant_sessions (
    id text NOT NULL,
    user_id text NOT NULL,
    room_id text NOT NULL,
    joined_at timestamp without time zone DEFAULT now(),
    left_at timestamp without time zone,
    last_seen timestamp without time zone DEFAULT now(),
    room_session_id text DEFAULT ''::text NOT NULL
);


ALTER TABLE public.participant_sessions OWNER TO postgres;

--
-- Name: participants; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.participants (
    id text NOT NULL,
    room_id text NOT NULL,
    name text,
    first_joined_at timestamp without time zone,
    last_seen timestamp without time zone
);


ALTER TABLE public.participants OWNER TO postgres;

--
-- Name: room_events; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.room_events (
    id integer NOT NULL,
    room_id text NOT NULL,
    session_id text NOT NULL,
    user_id text NOT NULL,
    event_type text NOT NULL,
    payload jsonb,
    created_at timestamp without time zone DEFAULT now()
);


ALTER TABLE public.room_events OWNER TO postgres;

--
-- Name: room_events_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.room_events_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.room_events_id_seq OWNER TO postgres;

--
-- Name: room_events_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.room_events_id_seq OWNED BY public.room_events.id;


--
-- Name: room_sessions; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.room_sessions (
    id text NOT NULL,
    room_id text NOT NULL,
    started_at timestamp without time zone DEFAULT now(),
    ended_at timestamp without time zone
);


ALTER TABLE public.room_sessions OWNER TO postgres;

--
-- Name: rooms; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.rooms (
    id character varying(50) NOT NULL,
    name character varying(100),
    created_at timestamp without time zone DEFAULT now()
);


ALTER TABLE public.rooms OWNER TO postgres;

--
-- Name: meeting_attendance id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.meeting_attendance ALTER COLUMN id SET DEFAULT nextval('public.meeting_attendance_id_seq'::regclass);


--
-- Name: room_events id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.room_events ALTER COLUMN id SET DEFAULT nextval('public.room_events_id_seq'::regclass);


--
-- Name: meeting_attendance meeting_attendance_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.meeting_attendance
    ADD CONSTRAINT meeting_attendance_pkey PRIMARY KEY (id);


--
-- Name: participant_sessions participant_sessions_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.participant_sessions
    ADD CONSTRAINT participant_sessions_pkey PRIMARY KEY (id);


--
-- Name: participants participants_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.participants
    ADD CONSTRAINT participants_pkey PRIMARY KEY (id, room_id);


--
-- Name: room_events room_events_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.room_events
    ADD CONSTRAINT room_events_pkey PRIMARY KEY (id);


--
-- Name: room_sessions room_sessions_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.room_sessions
    ADD CONSTRAINT room_sessions_pkey PRIMARY KEY (id);


--
-- Name: rooms rooms_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.rooms
    ADD CONSTRAINT rooms_pkey PRIMARY KEY (id);


--
-- PostgreSQL database dump complete
--

