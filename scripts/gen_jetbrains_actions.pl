#!/usr/bin/env perl
# Extract the JetBrains action registry into port/data/jetbrains_actions.json.
#
# The source is the IDE itself: every `<action id=… class=…>` registration in the
# plugin descriptors inside its jars. Keymap files also contain `<action id=…>`
# tags, but those carry no `class` — they bind shortcuts to actions registered
# elsewhere — so only tags with a `class` attribute count as an action.
#
# Display text comes from the tag's `text` attribute, falling back to the
# `action.<id>.text` key of the English resource bundles shipped beside it
# (ActionsBundle and friends), with mnemonic markers removed.
#
# Scope is the platform (Contents/lib) plus the editor-general bundled plugins in
# @PLUGINS. Language and framework plugins (Java, Kotlin, DatabaseTools, Docker,
# Kubernetes, …) are outside it.
#
# usage: scripts/gen_jetbrains_actions.pl ["/Applications/IntelliJ IDEA.app"]
use strict;
use warnings;
use File::Find;
use FindBin;
use JSON::PP;

my $APP = shift // '/Applications/IntelliJ IDEA.app';
my @PLUGINS = qw(
    vcs-git
    vcs-github
    platform-bookmarks-plugin
    terminal
    markdown
    platform-vcs-split-plugin
    platform-execution-serviceView-plugin
);

my $out = "$FindBin::Bin/../port/data/jetbrains_actions.json";

open my $pi, '<', "$APP/Contents/Resources/product-info.json" or die "$APP: no product-info.json\n";
my $product = decode_json(do { local $/; <$pi> });
my $build = "$product->{name} $product->{version} ($product->{buildNumber})";

sub jars_under {
    my ($dir) = @_;
    my @jars;
    find(sub { push @jars, $File::Find::name if /\.jar$/ }, $dir) if -d $dir;
    return sort @jars;
}

sub entries {
    my ($jar) = @_;
    open my $fh, '-|', 'unzip', '-Z1', $jar or return ();
    chomp(my @e = <$fh>);
    return @e;
}

sub member {
    my ($jar, $name) = @_;
    open my $fh, '-|:encoding(UTF-8)', 'unzip', '-p', $jar, $name or return '';
    local $/;
    return <$fh> // '';
}

sub attr {
    my ($tag, $name) = @_;
    return $tag =~ /\s\Q$name\E="([^"]*)"/ ? $1 : undef;
}

sub plain {
    my ($s) = @_;
    return undef unless defined $s && length $s;
    $s =~ s/\\u([0-9a-fA-F]{4})/chr hex $1/ge;
    $s =~ s/''/'/g;        # MessageFormat quote
    $s =~ s/&amp;/&/g;
    $s =~ s/&quot;/"/g;
    $s =~ s/&apos;/'/g;
    $s =~ s/&lt;/</g;
    $s =~ s/&gt;/>/g;
    $s =~ s/_(?=\w)//;     # mnemonic marker
    $s =~ s/&(?=\w)//;
    $s =~ s/\.\.\.$|\x{2026}$//;
    return $s;
}

my (%actions, %bundle);
my @scopes = (['platform', "$APP/Contents/lib"], map { [$_, "$APP/Contents/plugins/$_/lib"] } @PLUGINS);

for my $scope (@scopes) {
    my ($category, $dir) = @$scope;
    for my $jar (jars_under($dir)) {
        my $rel = $jar =~ s{^\Q$APP\E/}{}r;
        for my $name (entries($jar)) {
            if ($name =~ /Bundle\.properties$/) {
                for (split /\n/, member($jar, $name)) {
                    $bundle{$1} //= $2 if /^(action\.[^=\s]+\.(?:text|description))\s*=\s*(.*?)\s*$/;
                }
                next;
            }
            next unless $name =~ /\.xml$/ && $name !~ m{(^|/)keymaps?/};
            my $xml = member($jar, $name);
            while ($xml =~ /<action\b([^>]*)>/g) {
                my $tag = $1;
                my $id = attr($tag, 'id') // next;
                next unless defined attr($tag, 'class');
                $actions{$id} //= {
                    category => $category,
                    text     => attr($tag, 'text'),
                    desc     => attr($tag, 'description'),
                    doc_ref  => "$build: $rel!/$name",
                };
            }
        }
    }
}

my @items;
for my $id (sort keys %actions) {
    my $a = $actions{$id};
    my $text = plain($a->{text}) // plain($bundle{"action.$id.text"}) // $id;
    my $desc = plain($a->{desc}) // plain($bundle{"action.$id.description"});
    push @items, {
        id       => "jetbrains-actions.$id",
        source   => 'jetbrains-actions',
        category => $a->{category},
        name     => $text,
        desc     => defined $desc ? "$id: $desc" : $id,
        doc_ref  => $a->{doc_ref},
    };
}

open my $fh, '>', $out or die "$out: $!\n";
print $fh JSON::PP->new->utf8->canonical->pretty->indent_length(1)->encode(\@items);
close $fh;
printf STDERR "%d actions from %s -> %s\n", scalar @items, $build, $out;
